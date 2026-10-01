import { common } from '@kit.AbilityKit';
import { AppearanceMode } from '../platform/PlatformServices';
import {
  EndpointState,
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
   * Where an endpoint setup stands, in words.
   *
   * Each of these points at a different next action, which is the whole reason
   * they are separate. A wrong key and a wrong address are both "probe failed"
   * if the message is generic, and the user then edits whichever field they
   * happen to think of — usually the one that was not wrong.
   *
   * The three that read as success are not decoration. An endpoint with a
   * working protocol and no model list still chats, and telling the user that
   * is a problem would push them to abandon a configuration that works.
   */
  static endpointState(state: EndpointState, modelCount: number): ResourceStr {
    switch (state) {
      case EndpointState.NeedAddress:
        return $r('app.string.provider_need_address');
      case EndpointState.NeedKey:
        return $r('app.string.provider_need_key');
      case EndpointState.Ready:
        return $r('app.string.provider_ready');
      case EndpointState.Probing:
        return $r('app.string.provider_probing');
      case EndpointState.Usable:
        return $r('app.string.provider_probe_ok');
      case EndpointState.ModelsFound:
        // The count is the user's evidence that the list is the whole list. A
        // status that only said "models found" cannot be told apart from one
        // that quietly showed the first page of a longer list.
        return $r('app.string.provider_models_count', modelCount);
      case EndpointState.ListingUnavailable:
        return $r('app.string.provider_listing_unavailable');
      case EndpointState.AuthFailed:
        return $r('app.string.provider_auth_failed');
      case EndpointState.NoSuchEndpoint:
        return $r('app.string.provider_no_such_endpoint');
      case EndpointState.NetworkError:
        return $r('app.string.provider_network_error');
      case EndpointState.ServerError:
        return $r('app.string.provider_server_error');
      case EndpointState.RateLimited:
        return $r('app.string.provider_rate_limited');
      case EndpointState.NoUsableProtocol:
        return $r('app.string.provider_no_usable_protocol');
      case EndpointState.Undetermined:
        return $r('app.string.provider_undetermined');
      default:
        return $r('app.string.provider_unreadable');
    }
  }

  /**
   * The listing outcome in the platform's own language.
   *
   * The runtime sends a code here on purpose. It also sends an English
   * sentence, and that sentence is for the diagnostic log — printing it inside
   * a translated form produced exactly the failure this method exists to
   * remove: an untranslated clause sitting in the middle of a Chinese
   * sentence, which reads worse than either language alone and tells a Chinese
   * reader nothing about what to do next.
   *
   * Every non-`available` outcome ends by saying a model can still be typed,
   * because that is the part that decides whether the screen is a dead end.
   */
  static listingNote(listing: string): ResourceStr {
    switch (listing) {
      case 'available':
        return $r('app.string.provider_listing_available');
      case 'unavailable':
        return $r('app.string.provider_listing_absent');
      case 'permission_denied':
        return $r('app.string.provider_listing_permission_denied');
      case 'rate_limited':
        return $r('app.string.provider_listing_rate_limited');
      case 'unreachable':
        return $r('app.string.provider_listing_unreachable');
      default:
        return $r('app.string.provider_listing_unknown');
    }
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
      case 'provider_no_models':
        return $r('app.string.provider_no_models');
      case 'provider_need_address':
        return $r('app.string.provider_need_address');
      case 'turn_busy':
        return $r('app.string.error_turn_busy');
      case 'entity_not_found':
        return $r('app.string.error_entity_not_found');
      case 'already_removed':
        return $r('app.string.error_already_removed');
      case 'library_unavailable':
        return $r('app.string.error_library_unavailable');
      case 'save_failed':
        return $r('app.string.error_save_failed');
      case 'delete_failed':
        return $r('app.string.error_delete_failed');
      case 'bind_failed':
        return $r('app.string.error_bind_failed');
      default:
        return detail.length > 0 ? detail : $r('app.string.error_turn_failed');
    }
  }
}
