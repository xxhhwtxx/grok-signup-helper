import {invoke as nativeInvoke} from '@tauri-apps/api/core';
import {listen as nativeListen} from '@tauri-apps/api/event';
export const preview=false;
export const invoke=nativeInvoke;
export const listen=nativeListen;
