import { display } from '@kit.ArkUI';
import { window } from '@kit.ArkUI';

/**
 * The real system insets, measured from the window that is going to draw us.
 *
 * `setWindowLayoutFullScreen(true)` makes the window ignore the safe area so
 * content can reach the physical edge of the screen. That is the whole point:
 * the navigation indicator gives way to the page instead of the page stepping
 * around it. The cost is that nothing avoids the system bars any more, so the
 * page has to know how tall they are and pad itself.
 *
 * Why measured rather than guessed: a fixed number is wrong on every device
 * that is not the one it was written on. A status bar is not 24vp everywhere,
 * and a gesture indicator is not 48px on a device with three-button navigation
 * or with a different density. Reading the real rectangle is the only version
 * of this that survives being run somewhere else.
 *
 * Values are in vp because that is what layout attributes take, and the window
 * reports pixels.
 */
export class SafeArea {
  /** Status-bar height. Content that should not sit under the clock does. */
  static topVp: number = 0;

  /**
   * Navigation-indicator height.
   *
   * Zero on a device with no indicator, which is correct rather than a missing
   * measurement: there is nothing there to make room for.
   */
  static bottomVp: number = 0;

  /**
   * Reads both insets. Called once, before the first page is loaded, so the
   * first frame is already padded correctly and the page does not visibly jump
   * into place a moment after it appears.
   */
  static measure(win: window.Window): void {
    const density: number = display.getDefaultDisplaySync().densityPixels;
    const scale: number = density > 0 ? density : 1;
    SafeArea.topVp = SafeArea.topOf(win, scale);
    SafeArea.bottomVp = SafeArea.bottomOf(win, scale);
  }

  private static topOf(win: window.Window, scale: number): number {
    try {
      return win.getWindowAvoidArea(window.AvoidAreaType.TYPE_SYSTEM).topRect.height / scale;
    } catch (error) {
      // An inset we could not read is an inset we do not know. Padding by a
      // guess could hide the status bar or waste a strip; padding by nothing
      // at least leaves the failure visible instead of silently misplacing it.
      return 0;
    }
  }

  private static bottomOf(win: window.Window, scale: number): number {
    try {
      return win.getWindowAvoidArea(window.AvoidAreaType.TYPE_NAVIGATION_INDICATOR).bottomRect.height / scale;
    } catch (error) {
      return 0;
    }
  }
}
