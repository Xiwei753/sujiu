import {
  CharacterSummary,
  ContextSourceSummary,
  ConversationSnapshot,
  MessageRole,
  MessageSummary,
  ModelSummary,
  SessionSummary,
  SujiuBridge,
  TurnEvent,
  TurnEventKind,
  TurnEventListener
} from '../bridge/SujiuBridge';

interface ScriptStep {
  kind: TurnEventKind;
  text?: string;
  callId?: string;
  toolName?: string;
  summary?: string;
  isError?: boolean;
  message?: string;
}

/**
 * Preview data source used until `sujiu-ffi` exposes the conversation
 * surface. It implements the same contract the FFI bridge will implement, so
 * presentation and UI need no change when the real runtime lands.
 */
export class InMemorySujiuBridge implements SujiuBridge {
  private readonly models: ModelSummary[] = [
    { id: 'model-balanced', name: 'Sujiu Chat 3', providerLabel: 'Sujiu', contextLabel: '200k context' },
    { id: 'model-reasoning', name: 'Sujiu Reason 3', providerLabel: 'Sujiu', contextLabel: '200k context · thinking' },
    { id: 'model-local', name: 'Local Qwen 3', providerLabel: 'On device', contextLabel: '32k context' }
  ];

  private readonly characters: CharacterSummary[] = [
    {
      id: 'char-lin',
      name: 'Lin',
      tagline: 'Night-shift radio host who never says what she means',
      tags: ['Modern', 'Slow burn', 'Radio']
    },
    {
      id: 'char-qi',
      name: 'Qi',
      tagline: 'Archivist of a city that rewrote its own name',
      tags: ['Fantasy', 'Mystery']
    },
    {
      id: 'char-ayan',
      name: 'Ayan',
      tagline: 'Courier with a debt and a bicycle',
      tags: ['Modern', 'Adventure']
    },
    {
      id: 'char-mira',
      name: 'Mira',
      tagline: 'Cartographer mapping places that are not there yet',
      tags: ['Fantasy', 'Travel']
    }
  ];

  private readonly contextSources: ContextSourceSummary[] = [
    { id: 'world-lore', name: 'Rain City lorebook', kindLabel: 'World lore', recordCount: 42 },
    { id: 'story-event', name: 'Night of the broadcast', kindLabel: 'Story event', recordCount: 8 },
    { id: 'memory-lin', name: 'Lin · long term memory', kindLabel: 'Character memory', recordCount: 17 },
    { id: 'chat-history', name: 'Older conversations', kindLabel: 'Chat history', recordCount: 96 }
  ];

  private readonly sessions: SessionSummary[] = [
    {
      id: 'session-1',
      title: 'The 2 a.m. frequency',
      characterId: 'char-lin',
      characterName: 'Lin',
      preview: '…and the signal answers back.',
      updatedAt: 1756000000000
    },
    {
      id: 'session-2',
      title: 'Archive of the drowned district',
      characterId: 'char-qi',
      characterName: 'Qi',
      preview: 'The map is older than the street it describes.',
      updatedAt: 1755900000000
    },
    {
      id: 'session-3',
      title: 'A package, no return address',
      characterId: 'char-ayan',
      characterName: 'Ayan',
      preview: 'I counted the seals twice. Both times: nine.',
      updatedAt: 1755700000000
    },
    {
      id: 'session-4',
      title: 'Coastline that keeps moving',
      characterId: 'char-mira',
      characterName: 'Mira',
      preview: 'Every tide redraws the border.',
      updatedAt: 1755500000000
    }
  ];

  private readonly messages: MessageSummary[] = [
    {
      id: 'm-1',
      role: MessageRole.User,
      text: 'You said the frequency was dead. Why is the light on the console blinking?'
    },
    {
      id: 'm-2',
      role: MessageRole.Assistant,
      text: 'Because the console and I are arguing about who owns the night shift. ' +
        'The blinking is a caller who has not decided to speak yet.',
      toolName: 'search_context',
      toolStatusLabel: 'Searched 3 records'
    },
    {
      id: 'm-3',
      role: MessageRole.User,
      text: 'And if they decide to speak while I am on the line?'
    }
  ];

  private cancelledSessionId: string = '';

  listSessions(): Promise<SessionSummary[]> {
    return Promise.resolve(this.sessions);
  }

  listCharacters(query?: string): Promise<CharacterSummary[]> {
    const trimmed: string = (query ?? '').trim();
    if (trimmed.length === 0) {
      return Promise.resolve(this.characters);
    }
    const matched: CharacterSummary[] = this.characters.filter((character: CharacterSummary) => {
      return character.name.toLowerCase().includes(trimmed.toLowerCase()) ||
        character.tagline.toLowerCase().includes(trimmed.toLowerCase()) ||
        character.tags.some((tag: string) => tag.toLowerCase().includes(trimmed.toLowerCase()));
    });
    return Promise.resolve(matched);
  }

  listModels(): Promise<ModelSummary[]> {
    return Promise.resolve(this.models);
  }

  listContextSources(): Promise<ContextSourceSummary[]> {
    return Promise.resolve(this.contextSources);
  }

  conversationState(sessionId: string): Promise<ConversationSnapshot> {
    const found: SessionSummary | undefined = this.sessions.find((item: SessionSummary) => item.id === sessionId);
    const snapshot: ConversationSnapshot = {
      session: found ?? this.sessions[0],
      messages: this.messages,
      contextSources: this.contextSources
    };
    return Promise.resolve(snapshot);
  }

  sendTurn(sessionId: string, userText: string, onEvent: TurnEventListener): void {
    this.cancelledSessionId = '';
    onEvent({ kind: TurnEventKind.Started });
    this.emitScript(sessionId, this.scriptFor(userText), onEvent);
  }

  cancelTurn(sessionId: string): void {
    this.cancelledSessionId = sessionId;
  }

  runtimeSummary(): string {
    return 'Preview bridge · sujiu-ffi conversation API pending';
  }

  private emitScript(sessionId: string, steps: ScriptStep[], onEvent: TurnEventListener): void {
    let index: number = 0;
    const step: () => void = () => {
      if (this.cancelledSessionId === sessionId) {
        onEvent({ kind: TurnEventKind.Cancelled });
        return;
      }
      if (index >= steps.length) {
        onEvent({ kind: TurnEventKind.Completed });
        return;
      }
      const current: ScriptStep = steps[index];
      index += 1;
      onEvent({
        kind: current.kind,
        text: current.text,
        callId: current.callId,
        toolName: current.toolName,
        summary: current.summary,
        isError: current.isError,
        message: current.message
      });
      setTimeout(step, 24);
    };
    setTimeout(step, 24);
  }

  private scriptFor(userText: string): ScriptStep[] {
    if (userText.toLowerCase().includes('fail')) {
      return [
        { kind: TurnEventKind.TextDelta, text: 'The provider rejected the request' },
        { kind: TurnEventKind.Failed, message: 'Request failed: provider returned 429' }
      ];
    }
    return [
      { kind: TurnEventKind.ThinkingDelta, text: 'Recall the broadcast record, then answer in character.' },
      { kind: TurnEventKind.TextDelta, text: 'The console blinks because the night is not over yet. ' },
      { kind: TurnEventKind.TextDelta, text: 'You asked earlier whether I would let a caller in. ' },
      { kind: TurnEventKind.ToolCallStarted, callId: 'call-1', toolName: 'search_context' },
      {
        kind: TurnEventKind.ToolCallFinished,
        callId: 'call-1',
        toolName: 'search_context',
        summary: '3 records · 1 world lore, 1 story event, 1 memory',
        isError: false
      },
      { kind: TurnEventKind.TextDelta, text: 'I checked the night log, the lorebook and what I still owe you. ' },
      { kind: TurnEventKind.TextDelta, text: 'The answer is no, and I am going to let you stay anyway.' }
    ];
  }
}
