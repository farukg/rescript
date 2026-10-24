/* TypeScript file generated from CrossDirConsumer.res by genType. */

/* eslint-disable */
/* tslint:disable */

import * as CrossDirConsumerJS from './CrossDirConsumer.res.js';

import type {peer as CrossDirPeer_peer} from '../a/CrossDirPeer.gen';

export type bundle = { readonly peer: CrossDirPeer_peer; readonly note: string };

export const wrap: (peer:CrossDirPeer_peer, note:string) => bundle = CrossDirConsumerJS.wrap as any;
