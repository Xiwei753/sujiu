import { UIAbility } from '@kit.AbilityKit';
import { window } from '@kit.ArkUI';
import { AppGraph } from '../app/Controller';
import { SafeArea } from '../ui/SafeArea';

export default class EntryAbility extends UIAbility {
  onWindowStageCreate(windowStage: window.WindowStage): void {
    const main: window.Window = windowStage.getMainWindowSync();

    /**
     * Full screen, then content.
     *
     * `setWindowLayoutFullScreen(true)` stops the window from deciding the
     * safe area for us. Nothing about the page moves on this device — measured
     * insets are 33.92vp of status bar and 27.84vp of navigation indicator, and
     * the page's own padding lands within a pixel of where the window had put
     * it. That is the point, though, and it is worth being precise about: the
     * band below the composer was *already* our background before this change,
     * because the system bar is drawn over the window rather than beside it.
     * So full screen buys ownership, not pixels.
     *
     * What it did fix is the history drawer, whose title sat under the clock —
     * the drawer was the one surface not getting the window's avoidance.
     *
     * And what it deliberately does not do is let conversation content scroll
     * through the indicator band, which is what the reference implementation
     * this was copied from does. That needs the composer to float over the
     * list, and a message the user cannot read is a worse outcome than a strip
     * the page steps around. If that is ever wanted, it is its own decision.
     *
     * The order matters: loading first and switching after would show the whole
     * page shift once, at startup, which reads as a rendering fault.
     *
     * A refusal still loads the page. Coming up short of immersive is a
     * degraded layout; not coming up at all is not a trade worth making.
     */
    const load: () => void = (): void => {
      windowStage.loadContent('pages/Index').catch(() => {
        // The page layer owns its own error surface; a load failure here means
        // the ability cannot start at all, so there is nothing further to render.
      });
    };

    main
      .setWindowLayoutFullScreen(true)
      .then(() => {
        SafeArea.measure(main);
        load();
      })
      .catch(() => {
        load();
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
