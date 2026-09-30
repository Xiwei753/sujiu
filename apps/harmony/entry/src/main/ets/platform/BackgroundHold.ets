import { backgroundTaskManager } from '@kit.BackgroundTasksKit';
import { BusinessError } from '@kit.BasicServicesKit';

/**
 * Keeps a turn alive when the screen goes off.
 *
 * Locking the screen sends the app to the background — Huawei lists "locking
 * the screen" alongside returning home and switching apps as the operations that
 * do it. A suspended process loses its software and hardware resources, network
 * included, so a streaming turn dies mid-answer the moment the screen dims. From
 * the user's side that looks like the app losing the reply, and the reply is not
 * something that can be re-requested: the model has already been paid for it.
 *
 * It is not a permission problem, and that is worth being precise about.
 * `ohos.permission.INTERNET` is a normal_grant permission, granted at install,
 * and the `usedScene.when: "inuse"` on it is informational. Networking is
 * available right up until the process is suspended. The gap is the suspension,
 * not the grant.
 *
 * So the fix is a **transient task**: a short, bounded window in which the app
 * may finish the work it was already doing. The alternative — a continuous
 * background task — would need a long-running category, and a notification the
 * user can see. That is a real cost to impose on someone to protect a reply
 * that takes seconds, and the review guidance for continuous tasks is explicit
 * that the category must match the work rather than merely permit it. Finishing
 * an in-flight turn is precisely what a transient task is for.
 *
 * A refused hold is not an error. This system throws 9900001 (caller
 * verification) and 9900002 (transient task verification) when the request does
 * not qualify, and a chat turn that starts a tenth of a second after the screen
 * locked is not worth failing over. Every failure here is swallowed, and the
 * turn simply loses its guarantee.
 *
 * The window is not ours to size. The platform caps a transient task at three
 * minutes in the normal case and less on a low battery, and `DelaySuspendInfo`
 * carries back only the id and the duration it actually granted. So this class
 * does not schedule anything, does not extend anything, and does not pretend the
 * hold will last as long as a slow model might need. It asks once and gives the
 * window back when the turn ends, which is the whole of what a transient task
 * honestly offers. A turn that outlives the window dies the way it would have
 * anyway, and the transcript keeps every step it finished.
 */
export class BackgroundHold {
  private requestId: number = -1;

  /**
   * Ask for a short window to finish what is already running.
   *
   * Calling this when a hold is already held does nothing: the window is
   * already open, and a second request would only spend the platform's
   * willingness to grant one.
   */
  acquire(reason: string): void {
    if (this.requestId !== -1) {
      return;
    }
    try {
      const info: backgroundTaskManager.DelaySuspendInfo =
        backgroundTaskManager.requestSuspendDelay(reason, () => {
        // The window is closing on its own. Forgetting the id here means a
        // later release() cannot try to cancel something already gone.
        this.requestId = -1;
      });
      this.requestId = info.requestId;
    } catch (error) {
      const code: number = (error as BusinessError).code;
      if (code !== 9900001 && code !== 9900002) {
        // Anything else is a bug rather than a refusal, and a silent one: a
        // hold that never worked is indistinguishable from a turn that simply
        // finished. Logged, never thrown.
        console.warn(`background hold refused: ${code} ${(error as Error).message}`);
      }
      this.requestId = -1;
    }
  }

  /**
   * Give the window back as soon as the work it was protecting is done.
   *
   * Holding it longer would keep the process alive for a reply that has already
   * arrived, and every second of that is a second of battery the user paid for
   * nothing.
   */
  release(): void {
    if (this.requestId === -1) {
      return;
    }
    const requestId: number = this.requestId;
    this.requestId = -1;
    try {
      backgroundTaskManager.cancelSuspendDelay(requestId);
    } catch (error) {
      // The window may have expired between the turn ending and this call.
      // That is the outcome we wanted anyway.
    }
  }

  /** Whether a window is currently held. Read by tests and diagnostics. */
  get held(): boolean {
    return this.requestId !== -1;
  }
}
