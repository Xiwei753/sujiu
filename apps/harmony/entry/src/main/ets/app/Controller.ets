import { common } from '@kit.AbilityKit';
import { SujiuNativeBridge } from '../bridge/SujiuNativeBridge';
import { SujiuBridge } from '../bridge/SujiuBridge';
import {
  AppearanceMode,
  HarmonyClipboardService,
  HarmonyCredentialService,
  HarmonyFileService,
  HarmonyPlatformInfoService,
  HarmonySystemAppearanceService,
  PlatformServices
} from '../platform/PlatformServices';
import { ChatController } from '../presentation/ChatController';
import { AppTheme } from '../ui/theme/Theme';

/**
 * Composition root.
 *
 * The only place that knows concrete implementations. The bridge is the real
 * one: it talks to the shared Rust runtime through sujiu-ffi.
 *
 * Navigation is *not* here: pages are owned by ArkUI `Navigation` in the view
 * layer, and the runtime does not know a navigation stack exists.
 */
@Observed
export class AppGraph {
  readonly bridge: SujiuBridge;
  readonly platform: PlatformServices;
  readonly credentials: HarmonyCredentialService = new HarmonyCredentialService();
  readonly files: HarmonyFileService = new HarmonyFileService();
  readonly controller: ChatController;
  /** UI-layer tokens, kept in sync with the resolved appearance mode. */
  readonly theme: AppTheme = new AppTheme(false);
  private context: common.Context | null = null;

  constructor() {
    this.bridge = new SujiuNativeBridge();
    this.platform = new PlatformServices(
      new HarmonyClipboardService(),
      new HarmonySystemAppearanceService(),
      new HarmonyPlatformInfoService(),
      this.credentials,
      this.files
    );
    this.controller = new ChatController(this.bridge, this.platform, this.credentials);
    this.controller.onAppearanceChanged = () => this.syncTheme();
  }

  /** Bound once the UI is alive; the host context is only reachable there. */
  bind(context: common.Context): void {
    this.context = context;
    // Preferences need the application context, so the credential service is
    // bound before anything can ask it for a secret.
    this.credentials.bind(context);
    // The platform supplies the location, the runtime owns the document.
    const directory: string | null = this.files.dataDirectory(context);
    if (directory !== null) {
      this.bridge.useDataDirectory(directory);
    }
  }

  start(): void {
    this.syncTheme();
    this.controller.refresh();
    this.controller.loadProvider();
  }

  isDarkMode(): boolean {
    switch (this.controller.appearanceMode) {
      case AppearanceMode.Dark:
        return true;
      case AppearanceMode.Light:
        return false;
      default:
        return this.context !== null && this.platform.appearance.systemPrefersDark(this.context);
    }
  }

  /** Re-reads the platform appearance and re-points the theme tokens. */
  syncTheme(): void {
    this.theme.apply(this.isDarkMode());
  }
}
