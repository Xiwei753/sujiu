import { common } from '@kit.AbilityKit';
import { resourceManager } from '@kit.LocalizationKit';
import { deviceInfo, pasteboard } from '@kit.BasicServicesKit';

/**
 * Platform capability services.
 *
 * Capability verbs only (`copyText`, `systemPrefersDark`), never page verbs.
 * UI code must not call these directly: it asks presentation, which calls the
 * capability. Only services with a real caller are implemented today.
 */

export enum AppearanceMode {
  System = 'System',
  Light = 'Light',
  Dark = 'Dark'
}

export interface ClipboardService {
  copyText(label: string, text: string): void;
}

export class HarmonyClipboardService implements ClipboardService {
  copyText(label: string, text: string): void {
    const board = pasteboard.getSystemPasteboard();
    const data = pasteboard.createData(pasteboard.MIMETYPE_TEXT_PLAIN, text);
    board.setData(data);
  }
}

export interface SystemAppearanceService {
  systemPrefersDark(context: common.Context): boolean;
}

export class HarmonySystemAppearanceService implements SystemAppearanceService {
  systemPrefersDark(context: common.Context): boolean {
    return context.resourceManager.getConfigurationSync()
      .colorMode === resourceManager.ColorMode.DARK;
  }
}

export interface PlatformInfoService {
  platformSummary(): string;
}

export class HarmonyPlatformInfoService implements PlatformInfoService {
  platformSummary(): string {
    return `HarmonyOS · API ${deviceInfo.sdkApiVersion} · ${deviceInfo.brand}`;
  }
}

export class PlatformServices {
  readonly clipboard: ClipboardService;
  readonly appearance: SystemAppearanceService;
  readonly info: PlatformInfoService;

  constructor(clipboard: ClipboardService, appearance: SystemAppearanceService, info: PlatformInfoService) {
    this.clipboard = clipboard;
    this.appearance = appearance;
    this.info = info;
  }
}
