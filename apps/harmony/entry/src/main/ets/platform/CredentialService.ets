/*
 * Credential storage, as a platform service.
 *
 * The UI never opens a keystore, a preferences file or a keychain. It asks for
 * a secret by name and gets a string back; where that string lives is a
 * platform concern, which is why this interface lives in `platform/` and not in
 * a view or in the presentation layer.
 *
 * Known gap: HarmonyCredentialService stores secrets in the app's own
 * preferences, which is private to the app sandbox but not encrypted at rest.
 * The seam is in place so a HUKS-backed implementation can replace the storage
 * without any caller changing.
 */

import { preferences } from '@kit.ArkData';
import { common } from '@kit.AbilityKit';

export interface CredentialService {
  saveSecret(name: string, value: string): void;
  loadSecret(name: string): string;
  hasSecret(name: string): boolean;
  deleteSecret(name: string): void;
}

const STORE_NAME = 'sujiu_credentials';

export class HarmonyCredentialService implements CredentialService {
  private context: common.Context | null = null;
  private store: preferences.Preferences | null = null;

  /** Preferences need the application context, which the page hands over on start. */
  bind(context: common.Context): void {
    this.context = context;
    this.store = preferences.getPreferencesSync(context, { name: STORE_NAME });
  }

  saveSecret(name: string, value: string): void {
    const store = this.requireStore();
    store.putSync(name, value);
    store.flush();
  }

  loadSecret(name: string): string {
    const store = this.requireStore();
    const value = store.getSync(name, '') as string;
    return value;
  }

  hasSecret(name: string): boolean {
    return this.loadSecret(name).length > 0;
  }

  deleteSecret(name: string): void {
    const store = this.requireStore();
    store.deleteSync(name);
    store.flush();
  }

  private requireStore(): preferences.Preferences {
    if (this.store === null || this.context === null) {
      throw new Error('credential service is not bound to a context');
    }
    return this.store;
  }
}
