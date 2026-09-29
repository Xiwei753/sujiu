import { common } from '@kit.AbilityKit';
import { AppearanceMode } from '../platform/PlatformServices';
import {
  GenerationState,
  SessionGroupKey,
  ToolCallStatus
} from '../presentation/ChatController';

/**
 * Every user-visible string in the HarmonyOS frontend goes through here.
 *
 * Copy is a UI-layer concern. Presentation exposes stable states, groups and
 * failure codes instead of English sentences, the bridge reports codes rather
 * than prose, and Rust never invents display text. Translators therefore edit
 * one file per locale and never have to read ArkTS.
 *
 * `ResourceStr` keeps the lookup lazy: the platform resolves the entry for the
 * active locale when the text is rendered.
 */
export class Copy {
  /** Resolves a resource to a plain string, for APIs that reject ResourceStr. */
  static resolve(context: common.Context, resource: Resource): string {
    return context.resourceManager.getStringSync(resource.id);
  }

  static generationState(state: GenerationState): ResourceStr {
    switch (state) {
      case GenerationState.Submitting:
        return $r('app.string.status_submitting');
      case GenerationState.Streaming:
        return $r('app.string.status_streaming');
      case GenerationState.WaitingForTool:
        return $r('app.string.status_waiting_tool');
      case GenerationState.ExecutingTool:
        return $r('app.string.status_executing_tool');
      case GenerationState.ContinuingAfterTool:
        return $r('app.string.status_continuing');
      default:
        return '';
    }
  }

  static toolStatus(status: ToolCallStatus): ResourceStr {
    switch (status) {
      case ToolCallStatus.Queued:
        return $r('app.string.tool_status_queued');
      case ToolCallStatus.Running:
        return $r('app.string.tool_status_running');
      case ToolCallStatus.Failed:
        return $r('app.string.tool_status_failed');
      default:
        return $r('app.string.tool_status_done');
    }
  }

  static groupTitle(key: SessionGroupKey): ResourceStr {
    switch (key) {
      case SessionGroupKey.Today:
        return $r('app.string.history_group_today');
      case SessionGroupKey.Yesterday:
        return $r('app.string.history_group_yesterday');
      default:
        return $r('app.string.history_group_earlier');
    }
  }

  static appearanceMode(mode: AppearanceMode): ResourceStr {
    switch (mode) {
      case AppearanceMode.Light:
        return $r('app.string.appearance_light');
      case AppearanceMode.Dark:
        return $r('app.string.appearance_dark');
      default:
        return $r('app.string.appearance_system');
    }
  }

  /**
   * A session has no title of its own until the model produces one, so the
   * character name stands in, and "New chat" only when there is no character.
   */
  static sessionTitle(title: string, characterName: string): ResourceStr {
    if (title.length > 0) {
      return title;
    }
    if (characterName.length > 0) {
      return characterName;
    }
    return $r('app.string.sheet_item_new_chat');
  }

  /**
   * Failure codes come from presentation and the bridge, never from a
   * hardcoded sentence. An unrecognised code keeps the runtime's own text,
   * because an unexpected provider error is more useful raw than hidden.
   */
  static errorText(code: string, detail: string): ResourceStr {
    switch (code) {
      case 'new_chat_failed':
        return $r('app.string.error_new_chat_failed');
      case 'no_provider_configured':
        return $r('app.string.error_no_provider_configured');
      case 'missing_credential':
        return $r('app.string.error_missing_credential');
      case 'session_not_found':
        return $r('app.string.error_session_not_found');
      case 'provider_not_saved':
        return $r('app.string.error_provider_not_saved');
      case 'turn_busy':
        return $r('app.string.error_turn_busy');
      default:
        return detail.length > 0 ? detail : $r('app.string.error_turn_failed');
    }
  }
}
