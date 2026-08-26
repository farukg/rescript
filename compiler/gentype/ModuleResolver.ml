open GenTypeCommon
module ModuleNameMap = Map.Make (ModuleName)

let ( +++ ) = Filename.concat

(** Read all the dirs from a library in node_modules *)
let read_bs_dependencies_dirs ~root =
  let dirs = ref [] in
  let rec find_sub_dirs dir =
    let abs_dir =
      match dir = "" with
      | true -> root
      | false -> root +++ dir
    in
    if Sys.file_exists abs_dir && Sys.is_directory abs_dir then (
      dirs := dir :: !dirs;
      abs_dir |> Sys.readdir |> Array.iter (fun d -> find_sub_dirs (dir +++ d)))
  in
  find_sub_dirs "";
  !dirs

type pkgs = {dirs: string list; pkgs: (string, string) Hashtbl.t}

let read_dirs_from_config ~(config : Config.t) =
  let dirs = ref [] in
  let root = config.project_root in
  let ( +++ ) = Filename.concat in
  let rec process_dir ~subdirs dir =
    let abs_dir =
      match dir = "" with
      | true -> root
      | false -> root +++ dir
    in
    if Sys.file_exists abs_dir && Sys.is_directory abs_dir then (
      dirs := dir :: !dirs;
      if subdirs then
        abs_dir |> Sys.readdir
        |> Array.iter (fun d -> process_dir ~subdirs (dir +++ d)))
  in
  let rec process_source_item (source_item : Ext_json_types.t) =
    match source_item with
    | Str {str} -> str |> process_dir ~subdirs:false
    | Obj {map} -> (
      match Map_string.find_opt map "dir" with
      | Some (Str {str}) ->
        let subdirs =
          match Map_string.find_opt map "subdirs" with
          | Some (True _) -> true
          | Some (False _) -> false
          | _ -> false
        in
        str |> process_dir ~subdirs
      | _ -> ())
    | Arr {content} -> Array.iter process_source_item content
    | _ -> ()
  in
  (match config.sources with
  | Some source_item -> process_source_item source_item
  | None -> ());
  !dirs

let read_source_dirs ~(config : Config.t) =
  let source_dirs =
    ["lib"; "bs"; ".sourcedirs.json"]
    |> List.fold_left ( +++ ) config.bsb_project_root
  in
  let dirs = ref [] in
  let pkgs = Hashtbl.create 1 in
  let read_dirs json =
    match json with
    | Ext_json_types.Obj {map} -> (
      match Map_string.find_opt map "dirs" with
      | Some (Arr {content}) ->
        content
        |> Array.iter (fun x ->
               match x with
               | Ext_json_types.Str {str} -> dirs := str :: !dirs
               | _ -> ());
        ()
      | _ -> ())
    | _ -> ()
  in
  let read_pkgs json =
    match json with
    | Ext_json_types.Obj {map} -> (
      match Map_string.find_opt map "pkgs" with
      | Some (Arr {content}) ->
        content
        |> Array.iter (fun x ->
               match x with
               | Ext_json_types.Arr
                   {content = [|Str {str = name}; Str {str = path}|]} ->
                 Hashtbl.add pkgs name path
               | _ -> ());
        ()
      | _ -> ())
    | _ -> ()
  in
  if source_dirs |> Sys.file_exists then
    try
      let json = source_dirs |> Ext_json_parse.parse_json_from_file in
      if config.bsb_project_root <> config.project_root then
        dirs := read_dirs_from_config ~config
      else (
        read_dirs json;
        (* rewatch may omit per-package dirs or emit a shape without a usable
           `dirs` list; always fall back to rescript.json sources. *)
        if !dirs = [] then dirs := read_dirs_from_config ~config);
      read_pkgs json
    with _ -> dirs := read_dirs_from_config ~config
  else (
    Log_.item "Warning: can't find source dirs: %s\n" source_dirs;
    Log_.item "Falling back to rescript.json sources for genType module map.\n";
    dirs := read_dirs_from_config ~config);
  {dirs = !dirs; pkgs}

(** Read the project's .sourcedirs.json file if it exists
   and build a map of the files with the given extension
   back to the directory where they belong. *)
let sourcedirs_json_to_map ~config ~extensions ~exclude_file =
  let rec chop_extensions fname =
    match fname |> Filename.chop_extension with
    | fname_chopped -> fname_chopped |> chop_extensions
    | exception _ -> fname
  in
  let file_map = ref ModuleNameMap.empty in
  let bs_dependencies_file_map = ref ModuleNameMap.empty in
  let filter_given_extension file_name =
    extensions |> List.exists (fun ext -> Filename.check_suffix file_name ext)
    && not (exclude_file file_name)
  in
  let add_dir ~dir_on_disk ~dir_emitted ~filter ~map =
    dir_on_disk |> Sys.readdir
    |> Array.iter (fun fname ->
           if fname |> filter then
             map :=
               !map
               |> ModuleNameMap.add
                    (fname |> chop_extensions |> ModuleName.from_string_unsafe)
                    dir_emitted)
  in
  let {dirs; pkgs} = read_source_dirs ~config in
  dirs
  |> List.iter (fun dir ->
         add_dir ~dir_emitted:dir
           ~dir_on_disk:(config.project_root +++ dir)
           ~filter:filter_given_extension ~map:file_map);
  config.bs_dependencies
  |> List.iter (fun package_name ->
         match Hashtbl.find pkgs package_name with
         | path ->
           let root = ["lib"; "bs"] |> List.fold_left ( +++ ) path in
           let filter file_name =
             [".cmt"; ".cmti"]
             |> List.exists (fun ext -> Filename.check_suffix file_name ext)
           in
           read_bs_dependencies_dirs ~root
           |> List.iter (fun dir ->
                  let dir_on_disk = root +++ dir in
                  let dir_emitted = package_name +++ dir in
                  add_dir ~dir_emitted ~dir_on_disk ~filter
                    ~map:bs_dependencies_file_map)
         | exception Not_found -> ());
  (!file_map, !bs_dependencies_file_map)

type case = Lowercase | Uppercase

type resolver = {
  lazy_find:
    (use_bs_dependencies:bool -> ModuleName.t -> (string * case * bool) option)
    Lazy.t;
}

let create_lazy_resolver ~config ~extensions ~exclude_file =
  {
    lazy_find =
      lazy
        (let module_name_map, bs_dependencies_file_map =
           sourcedirs_json_to_map ~config ~extensions ~exclude_file
         in
         let find ~bs_dependencies ~map module_name =
           match map |> ModuleNameMap.find module_name with
           | resolved_module_dir ->
             Some (resolved_module_dir, Uppercase, bs_dependencies)
           | exception Not_found -> (
             match
               map |> ModuleNameMap.find (module_name |> ModuleName.uncapitalize)
             with
             | resolved_module_dir ->
               Some (resolved_module_dir, Lowercase, bs_dependencies)
             | exception Not_found -> None)
         in
         fun ~use_bs_dependencies module_name ->
           match
             module_name |> find ~bs_dependencies:false ~map:module_name_map
           with
           | None when use_bs_dependencies ->
             module_name
             |> find ~bs_dependencies:true ~map:bs_dependencies_file_map
           | res -> res);
  }

let apply ~resolver ~use_bs_dependencies module_name =
  module_name |> Lazy.force resolver.lazy_find ~use_bs_dependencies

(** Resolve a reference to ModuleName, and produce a path suitable for require.
   E.g. require "../foo/bar/ModuleName.ext" where ext is ".res" or ".js". *)
let resolve_module ~(config : Config.t) ~import_extension ~output_file_relative
    ~resolver ~use_bs_dependencies module_name =
  let output_file_relative_dir =
    (* e.g. src if we're generating src/File.bs.js *)
    Filename.dirname output_file_relative
  in
  let output_file_absolute_dir =
    config.project_root +++ output_file_relative_dir
  in
  let module_name_res_file =
    (* Check if the module is in the same directory as the file being generated.
       So if e.g. project_root/src/ModuleName.res exists. *)
    output_file_absolute_dir +++ (ModuleName.to_string module_name ^ ".res")
  in
  let candidate =
    (* e.g. import "./Modulename.ext" *)
    module_name
    |> ImportPath.from_module ~dir:Filename.current_dir_name ~import_extension
  in
  if Sys.file_exists module_name_res_file then candidate
  else
    (* Node-sep-safe directory of a node_rebase_file result (always "/"). *)
    let node_dirname path =
      match String.rindex_opt path '/' with
      | None -> Literals.node_current
      | Some 0 -> Literals.node_sep
      | Some i -> String.sub path 0 i
    in
    let relative_dir_from_emitter ~resolved_module_dir =
      Ext_path.node_rebase_file ~from:output_file_relative_dir
        ~to_:resolved_module_dir "x"
      |> node_dirname
    in
    (* Proactive project-tree rescue when the module map misses: scan
       rescript.json sources for ModuleName.res elsewhere in the package.
       In-project peers must never silently emit `./`. Externals still use
       the historical same-dir candidate. *)
    let rescue_in_project_module () =
      let module_base = ModuleName.to_string module_name in
      let module_base_lower = String.uncapitalize_ascii module_base in
      let rec search dirs =
        match dirs with
        | [] -> None
        | dir :: rest ->
          let try_name name =
            let abs = config.project_root +++ dir +++ (name ^ ".res") in
            match Sys.file_exists abs with
            | true -> Some (dir, name)
            | false -> None
          in
          match try_name module_base with
          | Some _ as hit -> hit
          | None -> (
            match try_name module_base_lower with
            | Some _ as hit -> hit
            | None -> search rest)
      in
      search (read_dirs_from_config ~config)
    in
    match module_name |> apply ~resolver ~use_bs_dependencies with
    | None -> (
      match rescue_in_project_module () with
      | Some (resolved_module_dir, case_name) ->
        if !Debug.module_resolution then
          Log_.item
            "Module map miss for %s; rescued from config sources at %s\n"
            (ModuleName.to_string module_name)
            resolved_module_dir;
        let from_output_dir_to_module_dir =
          relative_dir_from_emitter ~resolved_module_dir
        in
        ModuleName.from_string_unsafe case_name
        |> ImportPath.from_module ~dir:from_output_dir_to_module_dir
             ~import_extension
      | None ->
        (* External / unresolved: preserve historical `./Module.ext` candidate.
           Not a same-dir claim for an in-project peer (those are rescued above). *)
        if !Debug.module_resolution then
          Log_.item
            "Module map miss for %s; treating as external (./ candidate)\n"
            (ModuleName.to_string module_name);
        candidate)
    | Some (resolved_module_dir, case, bs_dependencies) ->
      (* Minimal relative dir from the emitter to the resolved module directory.
         Replaces project-root walk-up (`../../src/foo`) with a normal sibling
         relative path (`../foo`). *)
      let from_output_dir_to_module_dir =
        match bs_dependencies with
        | true -> resolved_module_dir
        | false -> relative_dir_from_emitter ~resolved_module_dir
      in
      (match case = Uppercase with
      | true -> module_name
      | false -> module_name |> ModuleName.uncapitalize)
      |> ImportPath.from_module ~dir:from_output_dir_to_module_dir
           ~import_extension

let resolve_generated_module ~config ~output_file_relative ~resolver module_name
    =
  if !Debug.module_resolution then
    Log_.item "Resolve Generated Module: %s\n"
      (module_name |> ModuleName.to_string);
  let import_path =
    resolve_module ~config
      ~import_extension:(ModuleExtension.generated_module_extension ~config)
      ~output_file_relative ~resolver ~use_bs_dependencies:true module_name
  in
  if !Debug.module_resolution then
    Log_.item "Import Path: %s\n" (import_path |> ImportPath.dump);
  import_path

(** Returns the path to import a given Reason module name. *)
let import_path_for_reason_module_name ~(config : Config.t)
    ~output_file_relative ~resolver module_name =
  if !Debug.module_resolution then
    Log_.item "Resolve Reason Module: %s\n" (module_name |> ModuleName.to_string);
  match config.shims_map |> ModuleNameMap.find module_name with
  | shim_module_name ->
    if !Debug.module_resolution then
      Log_.item "ShimModuleName: %s\n" (shim_module_name |> ModuleName.to_string);
    let import_extension =
      ModuleExtension.shim_ts_output_file_extension ~config
    in
    let import_path =
      resolve_module ~config ~import_extension ~output_file_relative ~resolver
        ~use_bs_dependencies:false shim_module_name
    in
    if !Debug.module_resolution then
      Log_.item "Import Path: %s\n" (import_path |> ImportPath.dump);
    import_path
  | exception Not_found ->
    module_name
    |> resolve_generated_module ~config ~output_file_relative ~resolver
