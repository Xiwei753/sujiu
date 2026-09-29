import { common } from '@kit.AbilityKit';

/**
 * Platform files capability.
 *
 * The runtime owns the document, the platform owns the location. Nothing here
 * reads or writes an application file: it only reports where a frontend may
 * keep one, and the runtime decides what the document contains.
 */
export interface FileService {
  /** A private directory this app may persist to, or null when unavailable. */
  dataDirectory(context: common.Context): string | null;
}

export class HarmonyFileService implements FileService {
  dataDirectory(context: common.Context): string | null {
    if (context === null || context === undefined) {
      return null;
    }
    const root: string | undefined = context.filesDir;
    return root === undefined || root.length === 0 ? null : root;
  }
}
