import { UIAbility } from '@kit.AbilityKit';
import { window } from '@kit.ArkUI';

export default class EntryAbility extends UIAbility {
  onWindowStageCreate(windowStage: window.WindowStage): void {
    windowStage.loadContent('pages/Index').catch(() => {
      // The page layer owns its own error surface; a load failure here means the
      // ability cannot start at all, so there is nothing further to render.
    });
  }
}
