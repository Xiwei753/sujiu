import { UIAbility } from '@kit.AbilityKit';
import { window } from '@kit.ArkUI';
import { AppGraph } from '../app/Controller';

export default class EntryAbility extends UIAbility {
  onWindowStageCreate(windowStage: window.WindowStage): void {
    windowStage.loadContent('pages/Index').catch(() => {
      // The page layer owns its own error surface; a load failure here means the
      // ability cannot start at all, so there is nothing further to render.
    });
  }

  /**
   * The app is leaving the foreground. Locking the screen is one of the ways
   * this happens.
   *
   * A suspended process gets no network at all, so a reply that was still being
   * written would simply stop. The ability knows about the lifecycle and nothing
   * about turns, so it says only that the app is going and lets the graph decide
   * whether anything is worth holding on to.
   */
  onBackground(): void {
    const graph: AppGraph | null = AppGraph.active();
    if (graph !== null) {
      graph.onBackground();
    }
  }

  onForeground(): void {
    const graph: AppGraph | null = AppGraph.active();
    if (graph !== null) {
      graph.onForeground();
    }
  }
}
