pub mod build_types;
pub mod clean;
pub mod compile;
pub mod compiler_info;
pub mod deps;
pub mod logs;
pub mod namespaces;
pub mod packages;
pub mod parse;
pub mod read_compile_state;

use self::parse::parser_args;
use crate::build::compile::{mark_modules_with_deleted_deps_dirty, mark_modules_with_expired_deps_dirty};
use crate::build::compiler_info::{CompilerCheckResult, verify_compiler_info, write_compiler_info};
use crate::helpers::emojis::*;
use crate::helpers::{self};
use crate::lock::{LockKind, drop_lock, get_lock_or_exit};
use crate::project_context::ProjectContext;
use crate::{config, sourcedirs};
use anyhow::{Context, Result, anyhow};
use build_types::*;
use console::style;
use indicatif::{ProgressBar, ProgressStyle};
use log::log_enabled;
use serde::Serialize;
use std::fmt;
use std::fs::File;
use std::io::Write;
use std::path::{Path, PathBuf};
use std::time::{Duration, Instant};

fn is_dirty(module: &Module) -> bool {
    match module.source_type {
        SourceType::SourceFile(SourceFile {
            implementation: Implementation {
                parse_dirty: true, ..
            },
            ..
        }) => true,
        SourceType::SourceFile(SourceFile {
            interface: Some(Interface {
                parse_dirty: true, ..
            }),
            ..
        }) => true,
        SourceType::SourceFile(_) => false,
        SourceType::MlMap(MlMap {
            parse_dirty: dirty, ..
        }) => dirty,
    }
}

#[derive(Serialize, Debug, Clone)]
pub struct CompilerArgs {
    pub compiler_args: Vec<String>,
    pub parser_args: Vec<String>,
}

pub fn get_compiler_args(rescript_file_path: &Path) -> Result<String> {
    let filename = &helpers::get_abs_path(rescript_file_path);
    let current_package = helpers::get_abs_path(
        &helpers::get_nearest_config(rescript_file_path).expect("Couldn't find package root"),
    );
    let project_context = ProjectContext::new(&current_package)?;

    let is_type_dev = match filename.strip_prefix(&current_package) {
        Err(_) => false,
        Ok(relative_path) => project_context
            .current_config
            .find_is_type_dev_for_path(relative_path),
    };

    // make PathBuf from package root and get the relative path for filename
    let relative_filename = filename.strip_prefix(PathBuf::from(&current_package)).unwrap();

    let file_path = PathBuf::from(&current_package).join(filename);
    let contents = helpers::read_file(&file_path).expect("Error reading file");

    let (ast_path, parser_args) = parser_args(
        &project_context,
        &project_context.current_config,
        relative_filename,
        &contents,
        /* is_local_dep */ true,
        /* warn_error_override */ None,
    )?;
    let is_interface = filename.to_string_lossy().ends_with('i');
    let has_interface = if is_interface {
        true
    } else {
        let mut interface_filename = filename.to_string_lossy().to_string();
        interface_filename.push('i');
        PathBuf::from(&interface_filename).exists()
    };
    let compiler_args = compile::compiler_args(
        &project_context.current_config,
        &ast_path,
        relative_filename,
        is_interface,
        has_interface,
        &project_context,
        &None,
        is_type_dev,
        true,
        None, // No warn_error_override for compiler-args command
    )?;

    let result = serde_json::to_string_pretty(&CompilerArgs {
        compiler_args,
        parser_args,
    })?;

    Ok(result)
}

pub fn get_compiler_info(project_context: &ProjectContext) -> Result<CompilerInfo> {
    let bsc_path = helpers::get_bsc();
    let bsc_hash = helpers::compute_file_hash(&bsc_path).ok_or(anyhow!(
        "Failed to compute bsc hash for {}",
        bsc_path.to_string_lossy()
    ))?;
    let runtime_path = compile::get_runtime_path(&project_context.current_config, project_context)?;
    Ok(CompilerInfo {
        bsc_path,
        bsc_hash,
        runtime_path,
    })
}

pub fn initialize_build(
    default_timing: Option<Duration>,
    filter: &Option<regex::Regex>,
    show_progress: bool,
    path: &Path,
    plain_output: bool,
    warn_error: Option<String>,
) -> Result<BuildCommandState> {
    let project_context = ProjectContext::new(path)?;
    let compiler = get_compiler_info(&project_context)?;

    let timing_clean_start = Instant::now();
    let packages = packages::make(filter, &project_context, show_progress)?;

    let compiler_check = verify_compiler_info(&packages, &compiler);

    if !packages::validate_packages_dependencies(&packages) {
        return Err(anyhow!("Failed to validate package dependencies"));
    }

    let mut build_state = BuildCommandState::new(
        path.to_path_buf(),
        project_context,
        packages,
        compiler,
        warn_error,
    );
    packages::parse_packages(&mut build_state)?;

    let compile_assets_state = read_compile_state::read(&mut build_state)?;

    let (diff_cleanup, total_cleanup) =
        clean::cleanup_previous_build(&mut build_state, compile_assets_state)?;
    compiler_info::invalidate_incompatible_build_state(&mut build_state)?;
    let timing_clean_total = timing_clean_start.elapsed();

    if show_progress {
        if plain_output {
            if let CompilerCheckResult::CleanedPackagesDueToCompiler = compiler_check {
                // Snapshot-friendly output (no progress prefixes or emojis)
                println!("Cleaned previous build due to compiler update");
            }
            println!("Cleaned {diff_cleanup}/{total_cleanup}")
        } else {
            if let CompilerCheckResult::CleanedPackagesDueToCompiler = compiler_check {
                println!(
                    "{}{} {}Cleaned previous build due to compiler update",
                    LINE_CLEAR,
                    style("[1/3]").bold().dim(),
                    SWEEP
                );
            }
            println!(
                "{}{} {}Cleaned {}/{} in {:.2}s",
                LINE_CLEAR,
                style("[1/3]").bold().dim(),
                SWEEP,
                diff_cleanup,
                total_cleanup,
                default_timing.unwrap_or(timing_clean_total).as_secs_f64()
            );
        }
    }

    Ok(build_state)
}

fn format_step(current: usize, total: usize) -> console::StyledObject<String> {
    style(format!("[{current}/{total}]")).bold().dim()
}

#[derive(Debug, Clone)]
pub enum IncrementalBuildErrorKind {
    SourceFileParseError,
    CompileError(Option<String>),
}

#[derive(Debug, Clone)]
pub struct IncrementalBuildError {
    pub plain_output: bool,
    pub kind: IncrementalBuildErrorKind,
}

impl fmt::Display for IncrementalBuildError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match &self.kind {
            IncrementalBuildErrorKind::SourceFileParseError => {
                if self.plain_output {
                    write!(f, "{LINE_CLEAR}  Could not parse Source Files",)
                } else {
                    write!(f, "{LINE_CLEAR}  {CROSS}Could not parse Source Files",)
                }
            }
            IncrementalBuildErrorKind::CompileError(Some(e)) => {
                if self.plain_output {
                    write!(f, "{LINE_CLEAR}  Failed to Compile. Error: {e}",)
                } else {
                    write!(f, "{LINE_CLEAR}  {CROSS}Failed to Compile. Error: {e}",)
                }
            }
            IncrementalBuildErrorKind::CompileError(None) => {
                if self.plain_output {
                    write!(f, "{LINE_CLEAR}  Failed to Compile. See Errors Above",)
                } else {
                    write!(f, "{LINE_CLEAR}  {CROSS}Failed to Compile. See Errors Above",)
                }
            }
        }
    }
}

pub fn with_build_lock<T>(path: &Path, f: impl FnOnce() -> T) -> T {
    let build_folder = path.to_string_lossy().to_string();
    let _lock = get_lock_or_exit(LockKind::Build, &build_folder);
    let result = f();
    let _lock = drop_lock(LockKind::Build, &build_folder);
    result
}

pub fn incremental_build(
    build_state: &mut BuildCommandState,
    default_timing: Option<Duration>,
    initial_build: bool,
    show_progress: bool,
    only_incremental: bool,
    create_sourcedirs: bool,
    plain_output: bool,
) -> Result<(), IncrementalBuildError> {
    let build_folder = build_state.root_folder.clone();
    with_build_lock(&build_folder, || {
        incremental_build_without_lock(
            build_state,
            default_timing,
            initial_build,
            show_progress,
            only_incremental,
            create_sourcedirs,
            plain_output,
            false,
        )
    })
}

// `build` needs to lock before initialization because initialization mutates previous
// build artifacts. Keep the compile body separate so it can run under that wider lock.
#[allow(clippy::too_many_arguments)]
pub fn incremental_build_without_lock(
    build_state: &mut BuildCommandState,
    default_timing: Option<Duration>,
    initial_build: bool,
    show_progress: bool,
    only_incremental: bool,
    create_sourcedirs: bool,
    plain_output: bool,
    // When true, abort on the first compile error. When false (the default),
    // keep scheduling every compilable module so all errors are collected.
    exit_after_first_error: bool,
) -> Result<(), IncrementalBuildError> {
    compile::mark_modules_with_missing_outputs_dirty(build_state).map_err(|e| IncrementalBuildError {
        kind: IncrementalBuildErrorKind::CompileError(Some(e.to_string())),
        plain_output,
    })?;
    logs::initialize(&build_state.packages);
    let num_dirty_modules = build_state.modules.values().filter(|m| is_dirty(m)).count() as u64;
    let pb = if !plain_output && show_progress {
        ProgressBar::new(num_dirty_modules)
    } else {
        ProgressBar::hidden()
    };
    let mut current_step = if only_incremental { 1 } else { 2 };
    let total_steps = if only_incremental { 2 } else { 3 };
    pb.set_style(
        ProgressStyle::with_template(&format!(
            "{} {}Parsing... {{spinner}} {{pos}}/{{len}} {{msg}}",
            format_step(current_step, total_steps),
            CODE
        ))
        .unwrap(),
    );

    let timing_parse_start = Instant::now();
    let timing_ast = Instant::now();
    let result_asts = parse::generate_asts(build_state, || pb.inc(1));
    let timing_ast_elapsed = timing_ast.elapsed();

    let parse_warnings = match result_asts {
        Ok(warnings) => {
            pb.finish();
            warnings
        }
        Err(err) => {
            logs::finalize(&build_state.packages);

            if !plain_output && show_progress {
                eprintln!(
                    "{}{} {}Error parsing source files in {:.2}s",
                    LINE_CLEAR,
                    format_step(current_step, total_steps),
                    CROSS,
                    default_timing.unwrap_or(timing_ast_elapsed).as_secs_f64()
                );
                pb.finish();
            }

            eprintln!("{}", err);

            return Err(IncrementalBuildError {
                kind: IncrementalBuildErrorKind::SourceFileParseError,
                plain_output,
            });
        }
    };
    let deleted_modules = build_state.deleted_modules.clone();
    deps::get_deps(build_state, &deleted_modules);
    let timing_parse_total = timing_parse_start.elapsed();

    if show_progress {
        if plain_output {
            println!("Parsed {num_dirty_modules} source files")
        } else {
            println!(
                "{}{} {}Parsed {} source files in {:.2}s",
                LINE_CLEAR,
                format_step(current_step, total_steps),
                CODE,
                num_dirty_modules,
                default_timing.unwrap_or(timing_parse_total).as_secs_f64()
            );
        }
    }
    if helpers::contains_ascii_characters(&parse_warnings) {
        eprintln!("{}", parse_warnings);
    }

    mark_modules_with_expired_deps_dirty(build_state).map_err(|e| IncrementalBuildError {
        kind: IncrementalBuildErrorKind::CompileError(Some(e.to_string())),
        plain_output,
    })?;
    mark_modules_with_deleted_deps_dirty(&mut build_state.build_state);
    current_step += 1;

    //print all the compile_dirty modules
    if log_enabled!(log::Level::Trace) {
        for (module_name, module) in build_state.modules.iter() {
            if module.compile_dirty {
                println!("compile dirty: {module_name}");
            }
        }
    };

    let start_compiling = Instant::now();
    let pb = if !plain_output && show_progress {
        ProgressBar::new(build_state.modules.len().try_into().unwrap())
    } else {
        ProgressBar::hidden()
    };
    pb.set_style(
        ProgressStyle::with_template(&format!(
            "{} {}Compiling... {{spinner}} {{pos}}/{{len}} {{msg}}",
            format_step(current_step, total_steps),
            SWORDS
        ))
        .unwrap(),
    );

    let (compile_errors, compile_warnings, num_compiled_modules) = compile::compile(
        build_state,
        show_progress,
        || pb.inc(1),
        |size| pb.set_length(size),
        exit_after_first_error,
    )
    .map_err(|e| IncrementalBuildError {
        kind: IncrementalBuildErrorKind::CompileError(Some(e.to_string())),
        plain_output,
    })?;

    let compile_duration = start_compiling.elapsed();

    logs::finalize(&build_state.packages);
    if create_sourcedirs {
        sourcedirs::print(build_state);
    }
    pb.finish();
    if !compile_errors.is_empty() {
        if show_progress {
            if plain_output {
                eprintln!("Compiled {num_compiled_modules} modules")
            } else {
                eprintln!(
                    "{}{} {}Compiled {} modules in {:.2}s",
                    LINE_CLEAR,
                    format_step(current_step, total_steps),
                    CROSS,
                    num_compiled_modules,
                    default_timing.unwrap_or(compile_duration).as_secs_f64()
                );
            }
        }
        if helpers::contains_ascii_characters(&compile_warnings) {
            eprintln!("{}", compile_warnings);
        }
        if initial_build {
            log_config_warnings(build_state);
        }
        if helpers::contains_ascii_characters(&compile_errors) {
            eprintln!("{}", compile_errors);
        }

        Err(IncrementalBuildError {
            kind: IncrementalBuildErrorKind::CompileError(None),
            plain_output,
        })
    } else {
        if show_progress {
            if plain_output {
                println!("Compiled {num_compiled_modules} modules")
            } else {
                println!(
                    "{}{} {}Compiled {} modules in {:.2}s",
                    LINE_CLEAR,
                    format_step(current_step, total_steps),
                    SWORDS,
                    num_compiled_modules,
                    default_timing.unwrap_or(compile_duration).as_secs_f64()
                );
            }
        }

        if helpers::contains_ascii_characters(&compile_warnings) {
            eprintln!("{}", compile_warnings);
        }
        if initial_build {
            log_config_warnings(build_state);
        }

        // Write per-package compiler metadata to `lib/bs/compiler-info.json` (idempotent)
        write_compiler_info(build_state);

        Ok(())
    }
}

fn log_config_warnings(build_state: &BuildCommandState) {
    build_state.packages.iter().for_each(|(_, package)| {
        // Only warn for local dependencies, not external packages
        if package.is_local_dep {
            package.config.get_deprecations().iter().for_each(
                |deprecation_warning| match deprecation_warning {
                    config::DeprecationWarning::BsDependencies => {
                        log_deprecated_config_field(&package.name, "bs-dependencies", "dependencies");
                    }
                    config::DeprecationWarning::BsDevDependencies => {
                        log_deprecated_config_field(&package.name, "bs-dev-dependencies", "dev-dependencies");
                    }
                    config::DeprecationWarning::BscFlags => {
                        log_deprecated_config_field(&package.name, "bsc-flags", "compiler-flags");
                    }
                    config::DeprecationWarning::PackageSpecsEs6 => {
                        log_deprecated_package_specs_module("es6");
                    }
                    config::DeprecationWarning::PackageSpecsEs6Global => {
                        log_deprecated_package_specs_module("es6-global");
                    }
                },
            );

            package
                .config
                .get_unsupported_fields()
                .iter()
                .for_each(|field| log_unsupported_config_field(&package.name, field));

            package
                .config
                .get_unknown_fields()
                .iter()
                .for_each(|field| log_unknown_config_field(&package.name, field));
        }
    });
}

fn log_deprecated_config_field(package_name: &str, field_name: &str, new_field_name: &str) {
    let warning = format!(
        "The field '{field_name}' found in the package config of '{package_name}' is deprecated and will be removed in a future version.\n\
        Use '{new_field_name}' instead."
    );
    eprintln!("\n{}", style(warning).yellow());
}

fn log_deprecated_package_specs_module(module_name: &str) {
    let warning = format!("deprecated: Option \"{module_name}\" is deprecated. Use \"esmodule\" instead.");
    eprintln!("\n{}", style(warning).yellow());
}

fn log_unsupported_config_field(package_name: &str, field_name: &str) {
    let warning = format!(
        "The field '{field_name}' found in the package config of '{package_name}' is not supported by ReScript 12's new build system."
    );
    eprintln!("\n{}", style(warning).yellow());
}

fn log_unknown_config_field(package_name: &str, field_name: &str) {
    let warning = format!(
        "Unknown field '{field_name}' found in the package config of '{package_name}'. This option will be ignored."
    );
    eprintln!("\n{}", style(warning).yellow());
}

// write build.ninja files in the packages after a non-incremental build
// this is necessary to bust the editor tooling cache. The editor tooling
// is watching this file.
// we don't need to do this in an incremental build because there are no file
// changes (deletes / additions)
pub fn write_build_ninja(build_state: &BuildCommandState) {
    for package in build_state.packages.values() {
        // write empty file:
        let mut f = File::create(package.get_build_path().join("build.ninja")).expect("Unable to write file");
        f.write_all(b"").expect("unable to write to ninja file");
    }
}

#[allow(clippy::too_many_arguments)]
pub fn build(
    filter: &Option<regex::Regex>,
    path: &Path,
    show_progress: bool,
    no_timing: bool,
    create_sourcedirs: bool,
    plain_output: bool,
    warn_error: Option<String>,
    exit_after_first_error: bool,
) -> Result<BuildCommandState> {
    let default_timing: Option<std::time::Duration> = if no_timing {
        Some(std::time::Duration::new(0.0 as u64, 0.0 as u32))
    } else {
        None
    };
    let timing_total = Instant::now();
    with_build_lock(path, || {
        let mut build_state = initialize_build(
            default_timing,
            filter,
            show_progress,
            path,
            plain_output,
            warn_error,
        )
        .with_context(|| "Could not initialize build")?;

        match incremental_build_without_lock(
            &mut build_state,
            default_timing,
            true,
            show_progress,
            false,
            create_sourcedirs,
            plain_output,
            exit_after_first_error,
        ) {
            Ok(_) => {
                if !plain_output && show_progress {
                    let timing_total_elapsed = timing_total.elapsed();
                    println!(
                        "\n{}{}Finished Compilation in {:.2}s",
                        LINE_CLEAR,
                        SPARKLES,
                        default_timing.unwrap_or(timing_total_elapsed).as_secs_f64()
                    );
                }
                clean::cleanup_after_build(&build_state);
                write_build_ninja(&build_state);
                Ok(build_state)
            }
            Err(e) => {
                clean::cleanup_after_build(&build_state);
                write_build_ninja(&build_state);
                Err(anyhow!("Incremental build failed. Error: {e}"))
            }
        }
    })
}
#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;
    use std::process;
    use std::sync::mpsc;
    use std::thread;
    use std::time::SystemTime;
    use tempfile::TempDir;

    #[test]
    fn with_build_lock_holds_lock_while_running_work() {
        let temp_dir = TempDir::new().expect("temp dir should be created");
        let project_folder = temp_dir.path();
        let build_lock_path = project_folder.join("lib").join(LockKind::Build.file_name());

        let result = with_build_lock(project_folder, || {
            assert!(
                build_lock_path.exists(),
                "build lock should be held while guarded work runs"
            );
            42
        });

        assert_eq!(result, 42);
        assert!(
            !build_lock_path.exists(),
            "build lock should be removed after guarded work finishes"
        );
    }

    #[test]
    fn with_build_lock_drops_lock_after_error_result() {
        let temp_dir = TempDir::new().expect("temp dir should be created");
        let project_folder = temp_dir.path();
        let build_lock_path = project_folder.join("lib").join(LockKind::Build.file_name());

        let result: Result<(), &str> = with_build_lock(project_folder, || Err("failed"));

        assert_eq!(result, Err("failed"));
        assert!(
            !build_lock_path.exists(),
            "build lock should be removed after guarded work returns an error"
        );
    }

    #[test]
    fn build_waits_for_lock_before_initializing() {
        let temp_dir = TempDir::new().expect("temp dir should be created");
        let project_folder = temp_dir.path().to_path_buf();
        let lib_dir = project_folder.join("lib");
        let build_lock_path = lib_dir.join(LockKind::Build.file_name());
        fs::create_dir_all(&lib_dir).expect("lib directory should be created");
        fs::write(&build_lock_path, process::id().to_string()).expect("lockfile should be written");

        let (sender, receiver) = mpsc::channel();
        let build_project_folder = project_folder.clone();
        let build_thread = thread::spawn(move || {
            // This temp project has no config, so initialization would fail immediately
            // if `build` did not wait for the lock first.
            let result = build(
                &None,
                &build_project_folder,
                false,
                true,
                false,
                true,
                None,
                false,
            );
            sender.send(result.is_err()).expect("result should be sent");
        });

        assert!(
            receiver.recv_timeout(Duration::from_millis(250)).is_err(),
            "build should wait for build.lock before running initialization"
        );

        fs::remove_file(&build_lock_path).expect("lockfile should be removed");
        assert!(
            receiver
                .recv_timeout(Duration::from_secs(5))
                .expect("build should finish after lock is removed")
        );
        build_thread.join().expect("build thread should complete");
    }

    fn build_output_fixture(root: &Path) -> BuildCommandState {
        build(&None, root, false, false, false, true, None, false).expect("fixture must compile")
    }

    fn file_snapshot(path: &Path) -> (Vec<u8>, SystemTime) {
        (
            fs::read(path).expect("fixture file must exist"),
            fs::metadata(path).unwrap().modified().unwrap(),
        )
    }

    fn assert_output_fixture_noop(root: &Path, outputs: &[PathBuf], phase: &str) {
        let snapshots = outputs.iter().map(|path| file_snapshot(path)).collect::<Vec<_>>();
        let state = build_output_fixture(root);
        for (path, snapshot) in outputs.iter().zip(snapshots) {
            assert_eq!(
                file_snapshot(path),
                snapshot,
                "{phase}: no-op rewrote {}",
                path.display()
            );
        }
        for (name, module) in &state.modules {
            match &module.source_type {
                SourceType::SourceFile(source) => {
                    assert_eq!(
                        source.implementation.compile_state,
                        CompileState::Pending,
                        "{name}"
                    );
                    assert!(!source.implementation.parse_dirty);
                    assert!(!module.compile_dirty);
                }
                SourceType::MlMap(_) => assert!(!module.compile_dirty),
            }
        }
    }

    #[test]
    fn provider_abi_change_rebuilds_unchanged_transitive_consumers() {
        let fixture = TempDir::new().unwrap();
        let root = fixture.path();
        let dependency = root.join("node_modules/abi-provider");
        fs::create_dir_all(root.join("src")).unwrap();
        fs::create_dir_all(dependency.join("src")).unwrap();
        fs::write(root.join("rescript.json"), r#"{"name":"abi-consumer","sources":["src"],"dependencies":["abi-provider"],"package-specs":{"module":"esmodule","in-source":true},"suffix":".mjs"}"#).unwrap();
        fs::write(dependency.join("rescript.json"), r#"{"name":"abi-provider","namespace":"Provider","sources":["src"],"package-specs":{"module":"esmodule","in-source":true},"suffix":".mjs"}"#).unwrap();
        fs::write(
            dependency.join("package.json"),
            r#"{"name":"abi-provider","type":"module"}"#,
        )
        .unwrap();
        let provider = dependency.join("src/State.res");
        fs::write(
            &provider,
            "type t = @as(0) Waiting(int) | @as(1) Ready(int)\nlet make = () => Ready(42)\nlet waiting = () => Waiting(99)\n",
        )
        .unwrap();
        fs::write(root.join("src/Alias.res"), "module State = Provider.State\n").unwrap();
        fs::write(root.join("src/Main.res"), "let read = value => switch value { | Alias.State.Waiting(_) => 0 | Alias.State.Ready(answer) => answer }\nlet answer = read(Alias.State.make())\n").unwrap();
        let run = |phase: &str, expected_tags: &str| {
            let output = process::Command::new("node")
                .args(["--input-type=module", "--eval", "import {answer, read} from './src/Main.mjs'; import {make, waiting} from './node_modules/abi-provider/src/State.mjs'; const tags = JSON.stringify([waiting().TAG, make().TAG]); if (tags !== process.argv[1] || answer !== 42 || read(waiting()) !== 0) { console.error({tags, answer, waiting: read(waiting())}); process.exit(1); } console.log(answer)", expected_tags])
                .current_dir(root)
                .output()
                .expect("Node must execute generated JavaScript");
            assert!(
                output.status.success(),
                "{phase}: {}",
                String::from_utf8_lossy(&output.stderr)
            );
            assert_eq!(String::from_utf8_lossy(&output.stdout).trim(), "42", "{phase}");
        };
        build_output_fixture(root);
        run("initial numeric ABI", "[0,1]");
        let consumers =
            ["src/Alias.res", "src/Main.res"].map(|path| (root.join(path), file_snapshot(&root.join(path))));
        let outputs = [
            "src/Alias.mjs",
            "src/Main.mjs",
            "node_modules/abi-provider/src/State.mjs",
        ]
        .map(|path| root.join(path));
        for rebuild_provider_separately in [false, true] {
            let before = fs::read(root.join("src/Main.mjs")).unwrap();
            let provider_before = fs::read(dependency.join("src/State.mjs")).unwrap();
            let source = match rebuild_provider_separately {
                false => {
                    "type t = @as(\"Waiting\") Waiting(int) | @as(\"Ready\") Ready(int)\nlet make = () => Ready(42)\nlet waiting = () => Waiting(99)\n"
                }
                true => {
                    "type t = @as(0) Waiting(int) | @as(1) Ready(int)\nlet make = () => Ready(42)\nlet waiting = () => Waiting(99)\n"
                }
            };
            let (phase, expected_tags) = match rebuild_provider_separately {
                false => ("root numeric-to-string ABI", r#"["Waiting","Ready"]"#),
                true => ("standalone string-to-numeric ABI", "[0,1]"),
            };
            fs::write(&provider, source).unwrap();
            if rebuild_provider_separately {
                build_output_fixture(&dependency);
                assert_ne!(
                    fs::read(dependency.join("src/State.mjs")).unwrap(),
                    provider_before,
                    "standalone provider must publish changed ABI"
                );
                let mut state = initialize_build(None, &None, false, root, true, None).unwrap();
                let deleted = state.deleted_modules.clone();
                deps::get_deps(&mut state, &deleted);
                compile::mark_modules_with_expired_deps_dirty(&mut state).unwrap();
                assert!(
                    state.modules["Main"].compile_dirty,
                    "provider publication must invalidate Main across Provider and Alias"
                );
            }
            build_output_fixture(root);
            run(phase, expected_tags);
            assert_ne!(fs::read(root.join("src/Main.mjs")).unwrap(), before, "{phase}");
            assert_ne!(
                fs::read(dependency.join("src/State.mjs")).unwrap(),
                provider_before,
                "{phase}: provider ABI output"
            );
            for (path, snapshot) in &consumers {
                assert_eq!(&file_snapshot(path), snapshot, "{phase}");
            }
            assert_output_fixture_noop(root, &outputs, phase);
            run(phase, expected_tags);
        }
    }

    #[test]
    fn fresh_rows_state_or_pattern_preserves_runtime_tags() {
        let fixture = TempDir::new().unwrap();
        let root = fixture.path();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("rescript.json"), r#"{"name":"rows-state-match","sources":["src"],"package-specs":{"module":"esmodule","in-source":true},"suffix":".mjs"}"#).unwrap();
        fs::write(root.join("src/Hooks.res"), "type rowsState<'row> = Init | Running(array<'row>) | Ready(array<'row>) | Failed(array<'row>, string)\nlet init = Init\nlet running = Running([7])\nlet ready = Ready([7])\nlet failed = Failed([7], \"failure\")\n").unwrap();
        fs::write(root.join("src/Consumer.res"), "let read = state => {\nlet (rows, settled) = switch state {\n| Hooks.Init => ([], false)\n| Running(rows) | Failed(rows, _) => (rows, false)\n| Ready(rows) => (rows, true)\n}\n(rows, settled)\n}\n").unwrap();
        build_output_fixture(root);
        let output = process::Command::new("node")
            .args(["--input-type=module", "--eval", "import * as Hooks from './src/Hooks.mjs'; import {read} from './src/Consumer.mjs'; for (const [name, rows, settled] of [['init', [], false], ['running', [7], false], ['ready', [7], true], ['failed', [7], false]]) { const result = read(Hooks[name]); if (JSON.stringify(result) !== JSON.stringify([rows, settled])) { console.error({name, state: Hooks[name], result}); process.exit(1); } } console.log('four states passed')"])
            .current_dir(root)
            .output()
            .expect("Node must execute fresh generated modules");
        assert!(
            output.status.success(),
            "{}\nHooks:\n{}\nConsumer:\n{}",
            String::from_utf8_lossy(&output.stderr),
            fs::read_to_string(root.join("src/Hooks.mjs")).unwrap(),
            fs::read_to_string(root.join("src/Consumer.mjs")).unwrap()
        );
        assert_eq!(
            String::from_utf8_lossy(&output.stdout).trim(),
            "four states passed"
        );
        let outputs = [root.join("src/Hooks.mjs"), root.join("src/Consumer.mjs")];
        assert_output_fixture_noop(root, &outputs, "fresh rows-state or-pattern");
    }

    #[test]
    fn legacy_build_state_reparses_without_deleting_outputs() {
        #[derive(serde::Serialize, serde::Deserialize)]
        struct LegacyCompilerInfo {
            version: String,
            bsc_path: String,
            bsc_hash: String,
            rescript_config_hash: String,
            runtime_path: String,
            generated_at: String,
        }
        let fixture = TempDir::new().unwrap();
        let root = fixture.path();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("rescript.json"), r#"{"name":"legacy-build-state","sources":["src"],"package-specs":{"module":"esmodule","in-source":true},"suffix":".mjs"}"#).unwrap();
        let source = root.join("src/Main.res");
        fs::write(&source, "let answer = 42\n").unwrap();
        build_output_fixture(root);
        let outputs = [
            root.join("src/Main.mjs"),
            root.join("lib/ocaml/Main.ast"),
            root.join("lib/ocaml/Main.cmt"),
        ];
        let snapshots = outputs.iter().map(|path| file_snapshot(path)).collect::<Vec<_>>();
        let source_snapshot = file_snapshot(&source);
        let metadata = root.join("lib/bs/compiler-info.json");
        let legacy: LegacyCompilerInfo = serde_json::from_slice(&fs::read(&metadata).unwrap()).unwrap();
        fs::write(&metadata, serde_json::to_vec(&legacy).unwrap()).unwrap();
        let state = initialize_build(None, &None, false, root, true, None).unwrap();
        let SourceType::SourceFile(main) = &state.modules["Main"].source_type else {
            panic!("Main is a source module");
        };
        assert!(main.implementation.parse_dirty);
        assert!(state.modules["Main"].compile_dirty);
        for (path, snapshot) in outputs.iter().zip(snapshots) {
            assert_eq!(
                file_snapshot(path),
                snapshot,
                "initialization must preserve {}",
                path.display()
            );
        }
        let rebuilt = build_output_fixture(root);
        let SourceType::SourceFile(main) = &rebuilt.modules["Main"].source_type else {
            panic!("Main is a source module");
        };
        assert_eq!(main.implementation.compile_state, CompileState::Success);
        assert_eq!(file_snapshot(&source), source_snapshot);
        assert_output_fixture_noop(root, &outputs, "upgraded build-state format");
    }

    #[test]
    fn empty_function_output_has_no_trailing_whitespace() {
        let fixture = TempDir::new().unwrap();
        let root = fixture.path();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("rescript.json"), r#"{"name":"empty-function-output","sources":["src"],"package-specs":{"module":"esmodule","in-source":true},"suffix":".mjs"}"#).unwrap();
        fs::write(root.join("src/Labels.res"), "type status = First | Second\nlet label = value => switch value { | First => None | Second => None }\nlet empty = () => ()\nlet nested = () => { let callback = () => (); callback }\nlet nonempty = value => value + 1\n").unwrap();
        build_output_fixture(root);
        let generated = root.join("src/Labels.mjs");
        let contents = fs::read_to_string(&generated).unwrap();
        for (line, text) in contents.lines().enumerate() {
            assert_eq!(text, text.trim_end(), "trailing whitespace at line {}", line + 1);
        }
        let output = process::Command::new("node")
            .args(["--input-type=module", "--eval", "import {label, empty, nested, nonempty} from './src/Labels.mjs'; if (label('First') !== undefined || label('Second') !== undefined || empty() !== undefined || nested()() !== undefined || nonempty(41) !== 42) process.exit(1)"])
            .current_dir(root)
            .output()
            .expect("Node must execute generated functions");
        assert!(
            output.status.success(),
            "{}",
            String::from_utf8_lossy(&output.stderr)
        );
        assert_output_fixture_noop(root, &[generated], "empty-function formatting");
    }

    #[test]
    fn identity_adapter_result_preserves_constructor_tags() {
        let fixture = TempDir::new().unwrap();
        let root = fixture.path();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("rescript.json"), r#"{"name":"adapter-result-match","sources":["src"],"package-specs":{"module":"esmodule","in-source":true},"suffix":".mjs"}"#).unwrap();
        fs::write(
            root.join("src/Adapter.res"),
            "type value<'a> = 'a\nlet constant = value => value\n",
        )
        .unwrap();
        fs::write(root.join("src/Hooks.res"), "module Rx = Adapter\ntype rowsState<'row> = Init | Running(array<'row>) | Ready(array<'row>) | Failed(array<'row>, string)\ntype handle<'row> = {state: Rx.value<rowsState<'row>>}\nlet useRowsState = (): Rx.value<rowsState<int>> => Rx.constant(Ready([7]))\n").unwrap();
        fs::write(root.join("src/Consumer.res"), "let read = () => {\nlet state = Hooks.useRowsState()\nlet (rows, settled) = switch state {\n| Hooks.Init => ([], false)\n| Running(rows) | Failed(rows, _) => (rows, false)\n| Ready(rows) => (rows, true)\n}\n(rows, settled)\n}\n").unwrap();
        build_output_fixture(root);
        let output = process::Command::new("node")
            .args(["--input-type=module", "--eval", "import {read} from './src/Consumer.mjs'; const actual = read(); if (JSON.stringify(actual) !== '[[7],true]') { console.error(actual); process.exit(1); } console.log('adapter tags passed')"])
            .current_dir(root)
            .output()
            .expect("Node must execute generated adapter result");
        assert!(
            output.status.success(),
            "{}\nConsumer:\n{}",
            String::from_utf8_lossy(&output.stderr),
            fs::read_to_string(root.join("src/Consumer.mjs")).unwrap()
        );
        assert_eq!(
            String::from_utf8_lossy(&output.stdout).trim(),
            "adapter tags passed"
        );
    }

    #[test]
    fn installed_cache_copy_preserves_original_package_outputs() {
        let fixture = TempDir::new().unwrap();
        let owner = fixture.path().join("owner");
        let app = fixture.path().join("nested/fixture");
        let installed = app.join("node_modules/copied-adapter");
        fs::create_dir_all(owner.join("src")).unwrap();
        fs::create_dir_all(app.join("src")).unwrap();
        fs::write(owner.join("rescript.json"), r#"{"name":"copied-adapter","sources":["src"],"package-specs":{"module":"esmodule","in-source":true},"suffix":".res.mjs"}"#).unwrap();
        fs::write(owner.join("src/Adapter.res"), "let answer = 42\n").unwrap();
        fs::write(owner.join("src/Adapter.resi"), "let answer: int\n").unwrap();
        build_output_fixture(&owner);
        let mut originals = Vec::new();
        let mut directories = vec![owner.clone()];
        while let Some(directory) = directories.pop() {
            for entry in fs::read_dir(directory).unwrap() {
                let entry = entry.unwrap();
                let path = entry.path();
                let file_type = entry.file_type().unwrap();
                if file_type.is_dir() {
                    directories.push(path);
                } else {
                    assert!(
                        file_type.is_file(),
                        "fixture must contain only directories and files"
                    );
                    let snapshot = file_snapshot(&path);
                    originals.push((path, snapshot));
                }
            }
        }
        for (path, _) in &originals {
            let target = installed.join(path.strip_prefix(&owner).unwrap());
            fs::create_dir_all(target.parent().unwrap()).unwrap();
            fs::copy(path, target).unwrap();
        }
        fs::write(app.join("rescript.json"), r#"{"name":"copied-adapter-consumer","sources":["src"],"dependencies":["copied-adapter"],"package-specs":{"module":"esmodule","in-source":true},"suffix":".res.mjs"}"#).unwrap();
        fs::write(app.join("src/Main.res"), "let answer = Adapter.answer\n").unwrap();
        build_output_fixture(&app);
        for (path, snapshot) in &originals {
            assert_eq!(
                &file_snapshot(path),
                snapshot,
                "installed build changed original {}",
                path.display()
            );
        }
        let outputs = [
            app.join("src/Main.res.mjs"),
            installed.join("src/Adapter.res.mjs"),
            installed.join("lib/ocaml/Adapter.ast"),
            installed.join("lib/ocaml/Adapter.iast"),
        ];
        assert_output_fixture_noop(&app, &outputs, "installed package cache relocation");
        for (path, snapshot) in &originals {
            assert_eq!(
                &file_snapshot(path),
                snapshot,
                "installed no-op changed original {}",
                path.display()
            );
        }
    }

    #[test]
    fn obsolete_hyphen_outputs_retire_without_deleting_live_owners() {
        for (namespace, specs, old_outputs) in [
            (
                "false",
                r#"{"module":"esmodule","in-source":true}"#,
                vec!["src/default.res.mjs"],
            ),
            (
                "false",
                r#"[{"module":"esmodule","in-source":false,"suffix":".mjs"},{"module":"commonjs","in-source":false,"suffix":".cjs"}]"#,
                vec!["lib/es6/src/default.mjs", "lib/js/src/default.cjs"],
            ),
            (
                r#""OutputScope""#,
                r#"{"module":"esmodule","in-source":true}"#,
                vec!["src/default-Facet.res.mjs"],
            ),
        ] {
            let fixture = TempDir::new().unwrap();
            let root = fixture.path();
            fs::create_dir_all(root.join("src")).unwrap();
            fs::write(root.join("rescript.json"), format!(r#"{{"name":"output-retirement","namespace":{namespace},"sources":["src"],"package-specs":{specs},"suffix":".res.mjs"}}"#)).unwrap();
            let old = root.join("src/default-Facet.res");
            fs::write(&old, "let answer = 42\n").unwrap();
            build_output_fixture(root);
            let outputs = old_outputs.iter().map(|path| root.join(path)).collect::<Vec<_>>();
            assert_output_fixture_noop(root, &outputs, "hyphen output follows actual BSC prefix");
            fs::rename(&old, root.join("src/Config_Default_Facet.res")).unwrap();
            build_output_fixture(root);
            for output in &outputs {
                assert!(!output.exists(), "obsolete JS remains: {}", output.display());
            }
            let basename = match namespace {
                "false" => "default-Facet",
                _ => "default-Facet-OutputScope",
            };
            for extension in ["cmi", "cmj", "cmt"] {
                for directory in ["lib/ocaml", "lib/bs/src"] {
                    assert!(
                        !root
                            .join(directory)
                            .join(format!("{basename}.{extension}"))
                            .exists(),
                        "obsolete {extension} remains in {directory}"
                    );
                }
            }
            for directory in ["lib/ocaml", "lib/bs/src"] {
                assert!(!root.join(directory).join("default-Facet.ast").exists());
            }
            let replacements = outputs
                .iter()
                .map(|path| {
                    path.with_file_name(match path.extension().unwrap().to_str().unwrap() {
                        "cjs" => "Config_Default_Facet.cjs",
                        "mjs" if specs.contains("in-source\":true") => "Config_Default_Facet.res.mjs",
                        "mjs" => "Config_Default_Facet.mjs",
                        extension => panic!("Unexpected test extension {extension}"),
                    })
                })
                .collect::<Vec<_>>();
            assert_output_fixture_noop(root, &replacements, "renamed output retirement");
        }

        let fixture = TempDir::new().unwrap();
        let root = fixture.path();
        fs::create_dir_all(root.join("src")).unwrap();
        fs::write(root.join("rescript.json"), r#"{"name":"live-output-owner","sources":["src"],"package-specs":{"module":"esmodule","in-source":true},"suffix":".res.mjs"}"#).unwrap();
        fs::write(root.join("src/default-Facet.res"), "let answer = 42\n").unwrap();
        build_output_fixture(root);
        fs::write(root.join("src/default.res"), "let answer = 7\n").unwrap();
        build_output_fixture(root);
        let output = root.join("src/default.res.mjs");
        let snapshot = file_snapshot(&output);
        fs::rename(root.join("src/default-Facet.res"), root.join("retired-Facet.res")).unwrap();
        initialize_build(None, &None, false, root, true, None).unwrap();
        assert_eq!(
            file_snapshot(&output),
            snapshot,
            "cleanup deleted a current source's output"
        );
        build_output_fixture(root);
        assert_output_fixture_noop(root, &[output], "surviving output owner");
    }

    #[test]
    fn missing_outputs_rebuild_without_source_changes() {
        for (specs, suffix, js_paths) in [
            (
                r#"{"module":"esmodule","in-source":true}"#,
                ".res.mjs",
                vec![
                    "src/nested/Main.res.mjs",
                    "node_modules/output-dep/src/nested/Value.res.mjs",
                ],
            ),
            (
                r#"{"module":"commonjs","in-source":false,"suffix":".cjs"}"#,
                ".ignored.js",
                vec![
                    "lib/js/src/nested/Main.cjs",
                    "node_modules/output-dep/lib/js/src/nested/Value.cjs",
                ],
            ),
            (
                r#"[{"module":"esmodule","in-source":false,"suffix":".mjs"},{"module":"commonjs","in-source":false,"suffix":".js"}]"#,
                ".ignored.js",
                vec![
                    "lib/es6/src/nested/Main.mjs",
                    "node_modules/output-dep/lib/es6/src/nested/Value.mjs",
                    "lib/js/src/nested/Main.js",
                    "node_modules/output-dep/lib/js/src/nested/Value.js",
                ],
            ),
        ] {
            let fixture = TempDir::new().unwrap();
            let root = fixture.path();
            let dependency = root.join("node_modules/output-dep");
            fs::create_dir_all(root.join("src/nested")).unwrap();
            fs::create_dir_all(dependency.join("src/nested")).unwrap();
            fs::write(
                root.join("rescript.json"),
                format!(
                    r#"{{"name":"output-root","sources":[{{"dir":"src","subdirs":true}}],"dependencies":["output-dep"],"package-specs":{specs},"suffix":"{suffix}"}}"#
                ),
            )
            .unwrap();
            fs::write(
                dependency.join("rescript.json"),
                r#"{"name":"output-dep","namespace":"OutputDep","sources":[{"dir":"src","subdirs":true}],"package-specs":{"module":"commonjs","in-source":true},"suffix":".dependency.js"}"#,
            )
            .unwrap();
            fs::write(
                root.join("src/nested/Main.res"),
                "let answer = OutputDep.Value.answer\n",
            )
            .unwrap();
            fs::write(dependency.join("src/nested/Value.res"), "let answer = 42\n").unwrap();
            fs::write(dependency.join("src/nested/Value.resi"), "let answer: int\n").unwrap();

            build_output_fixture(root);
            let inputs = [
                "src/nested/Main.res",
                "node_modules/output-dep/src/nested/Value.res",
                "node_modules/output-dep/src/nested/Value.resi",
                "lib/ocaml/Main.ast",
                "node_modules/output-dep/lib/ocaml/Value.ast",
                "node_modules/output-dep/lib/ocaml/Value.iast",
            ]
            .map(|path| (root.join(path), file_snapshot(&root.join(path))));

            let mut outputs = js_paths.iter().map(|path| root.join(path)).collect::<Vec<_>>();
            for extension in ["cmi", "cmj", "cmt"] {
                outputs.extend([
                    root.join(format!("lib/ocaml/Main.{extension}")),
                    root.join(format!("lib/bs/src/nested/Main.{extension}")),
                    dependency.join(format!("lib/ocaml/Value-OutputDep.{extension}")),
                    dependency.join(format!("lib/bs/src/nested/Value-OutputDep.{extension}")),
                    dependency.join(format!("lib/ocaml/OutputDep.{extension}")),
                    dependency.join(format!("lib/bs/OutputDep.{extension}")),
                ]);
            }
            outputs.extend([
                dependency.join("lib/ocaml/Value-OutputDep.cmti"),
                dependency.join("lib/bs/src/nested/Value-OutputDep.cmti"),
            ]);
            assert_output_fixture_noop(root, &outputs, "initial build");
            let javascript = js_paths
                .iter()
                .map(|path| (root.join(path), fs::read(root.join(path)).unwrap()))
                .collect::<Vec<_>>();

            for output in &outputs {
                assert!(output.is_file(), "compiler did not produce {}", output.display());
                fs::remove_file(output).unwrap();
                build_output_fixture(root);
                assert!(output.is_file(), "build did not restore {}", output.display());
                for (input, snapshot) in &inputs {
                    assert_eq!(&file_snapshot(input), snapshot, "{} changed", input.display());
                }
                for (path, bytes) in &javascript {
                    assert_eq!(&fs::read(path).unwrap(), bytes);
                }
                assert_output_fixture_noop(root, &outputs, &format!("restored {}", output.display()));
            }
        }
    }
}
