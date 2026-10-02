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
import { BackgroundHold } from '../platform/BackgroundHold';
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
  /**
   * The graph the ability can reach.
   *
   * Pages are created by ArkUI, not by the ability, so the ability has no
   * constructor argument through which to receive the graph the page built. It
   * reaches it here instead. This is the one place a page-owned object is
   * published, and it exists only so a lifecycle callback can find it.
   */
  private static instance: AppGraph | null = null;

  readonly bridge: SujiuBridge;
  readonly platform: PlatformServices;
  readonly credentials: HarmonyCredentialService = new HarmonyCredentialService();
  readonly files: HarmonyFileService = new HarmonyFileService();
  readonly controller: ChatController;
  /** UI-layer tokens, kept in sync with the resolved appearance mode. */
  readonly theme: AppTheme = new AppTheme(false);
  private context: common.Context | null = null;
  /**
   * Keeps a turn alive across the moment the screen turns off.
   *
   * Held here because the lifecycle belongs to this layer: the ability says the
   * app went to the background, the platform asks for a hold, and the
   * presentation layer only reports when a turn is over.
   */
  private readonly backgroundHold: BackgroundHold = new BackgroundHold();

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
    this.controller.onTurnSettled = () => this.backgroundHold.release();
    AppGraph.instance = this;
  }

  /**
   * The graph a lifecycle callback should talk to, or null before a page has
   * built one. A background event that arrives first is dropped rather than
   * queued: there is no turn to protect yet, and holding for one would need a
   * turn that does not exist.
   */
  static active(): AppGraph | null {
    return AppGraph.instance;
  }

  /**
   * The app is going to the background. Locking the screen is one of the ways
   * this happens, and a suspended process gets no network, so a turn in flight
   * would stop mid-answer. The hold buys it a short window to finish.
   *
   * Only asked for while a turn is actually running. Holding otherwise would
   * keep the process alive to protect nothing.
   */
  onBackground(): void {
    if (!this.controller.busy) {
      return;
    }
    this.backgroundHold.acquire('A reply is still being written.');
  }

  /** The app came back. The hold is already gone if the turn finished. */
  onForeground(): void {
    this.backgroundHold.release();
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

  /**
   * Brings the app up.
   *
   * Returns a promise because a caller may need the loaded state to be *there*
   * before it acts on it. A settings form that opened before the stored endpoint
   * had been read would show empty fields, which is the same as showing none
   * configured.
   */
  async start(): Promise<void> {
    this.syncTheme();
    this.controller.refresh();
    await this.controller.loadProvider();
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

  /**
   * Re-reads the platform appearance and applies it.
   *
   * The context is passed through so `setColorMode` can force the resolved
   * appearance. Every colour in the theme is a `sys.color.*` resource, and
   * those resolve against the application's colour mode — so a user's choice of
   * Light or Dark has to be told to the platform, not merely recorded.
   */
  syncTheme(): void {
    this.theme.apply(this.isDarkMode(), this.context ?? undefined);
  }
}
