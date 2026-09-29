import { common } from '@kit.AbilityKit';
import { InMemorySujiuBridge } from '../bridge/InMemorySujiuBridge';
import { SujiuBridge } from '../bridge/SujiuBridge';
import {
  AppearanceMode,
  HarmonyClipboardService,
  HarmonyPlatformInfoService,
  HarmonySystemAppearanceService,
  PlatformServices
} from '../platform/PlatformServices';
import { ChatController } from '../presentation/ChatController';
import { AppTheme } from '../ui/theme/Theme';

/**
 * Composition root.
 *
 * The only place that knows concrete implementations. Swapping the preview
 * bridge for the `sujiu-ffi` bridge is a one-line change here.
 *
 * Navigation is *not* here: pages are owned by ArkUI `Navigation` in the view
 * layer, and the runtime does not know a navigation stack exists.
 */
@Observed
export class AppGraph {
  readonly bridge: SujiuBridge;
  readonly platform: PlatformServices;
  readonly controller: ChatController;
  /** UI-layer tokens, kept in sync with the resolved appearance mode. */
  readonly theme: AppTheme = new AppTheme(false);
  private context: common.Context | null = null;

  constructor() {
    this.bridge = new InMemorySujiuBridge();
    this.platform = new PlatformServices(
      new HarmonyClipboardService(),
      new HarmonySystemAppearanceService(),
      new HarmonyPlatformInfoService()
    );
    this.controller = new ChatController(this.bridge, this.platform);
    this.controller.onAppearanceChanged = () => this.syncTheme();
  }

  /** Bound once the UI is alive; the host context is only reachable there. */
  bind(context: common.Context): void {
    this.context = context;
  }

  start(): void {
    this.syncTheme();
    this.controller.refresh();
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
