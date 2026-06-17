import type { LlmProvider } from '@puppet-master/shared';

export type ChatRole = 'user' | 'assistant' | 'system';
export type StopReason = 'end_turn' | 'tool_use' | 'max_tokens' | 'stop_sequence';

export interface TextBlock {
  type: 'text';
  text: string;
}
export interface ToolUseBlock {
  type: 'tool_use';
  id: string;
  name: string;
  input: unknown;
}
export type AssistantBlock = TextBlock | ToolUseBlock;

export interface UserMessage {
  role: 'user';
  content: string | Array<{ type: 'tool_result'; tool_use_id: string; content: string; is_error?: boolean }>;
}

export interface AssistantMessage {
  role: 'assistant';
  content: AssistantBlock[];
  stop_reason?: StopReason;
}

export type ChatMessage = UserMessage | AssistantMessage | { role: 'system'; content: string };

/** Normalize assistant message content from UI history or API quirks. */
export function assistantBlocks(content: unknown): AssistantBlock[] {
  if (Array.isArray(content)) {
    return content.filter(
      (b): b is AssistantBlock =>
        typeof b === 'object' &&
        b !== null &&
        'type' in b &&
        (b.type === 'text' || b.type === 'tool_use'),
    );
  }
  if (typeof content === 'string' && content.length > 0) {
    return [{ type: 'text', text: content }];
  }
  return [];
}

/** Extract plain text from OpenAI-compatible message content (string or parts array). */
function openAiMessageText(content: unknown): string {
  if (typeof content === 'string') return content;
  if (!Array.isArray(content)) return '';
  return content
    .map((part) => {
      if (typeof part === 'string') return part;
      if (typeof part === 'object' && part !== null && 'text' in part) {
        return String((part as { text?: unknown }).text ?? '');
      }
      return '';
    })
    .filter(Boolean)
    .join('\n');
}

export interface ToolDef {
  name: string;
  description: string;
  input_schema: Record<string, unknown>;
}

export interface LlmRequest {
  model: string;
  system: string;
  messages: ChatMessage[];
  tools: ToolDef[];
  max_tokens?: number;
}

export interface LlmResponse {
  content: AssistantBlock[];
  stop_reason: StopReason;
  usage?: { input_tokens: number; output_tokens: number };
}

export interface LlmStreamCallbacks {
  onTextDelta: (text: string) => void;
  signal?: AbortSignal;
}

/* Anthropic Messages API */
const ANTHROPIC_URL = 'https://api.anthropic.com/v1/messages';

function anthropicMessages(req: LlmRequest) {
  return req.messages
    .filter((m) => m.role !== 'system')
    .map((m) => {
      if (m.role === 'assistant') {
        return { role: 'assistant' as const, content: assistantBlocks(m.content) };
      }
      return m;
    });
}

export async function callAnthropic(apiKey: string, req: LlmRequest): Promise<LlmResponse> {
  const body = {
    model: req.model,
    system: req.system,
    messages: anthropicMessages(req),
    tools: req.tools,
    max_tokens: req.max_tokens ?? 4096,
  };
  const res = await fetch(ANTHROPIC_URL, {
    method: 'POST',
    headers: {
      'content-type': 'application/json',
      'x-api-key': apiKey,
      'anthropic-version': '2023-06-01',
      'anthropic-dangerous-direct-browser-access': 'true',
    },
    body: JSON.stringify(body),
  });
  if (!res.ok) {
    const text = await res.text();
    throw new Error(`anthropic ${res.status}: ${text}`);
  }
  const j = await res.json();
  return {
    content: j.content as AssistantBlock[],
    stop_reason: j.stop_reason,
    usage: j.usage,
  };
}

async function parseSseStream(
  body: ReadableStream<Uint8Array>,
  onEvent: (event: string, data: string) => void,
  signal?: AbortSignal,
): Promise<void> {
  const reader = body.getReader();
  const decoder = new TextDecoder();
  let buffer = '';

  try {
    while (true) {
      if (signal?.aborted) throw new DOMException('Aborted', 'AbortError');
      const { done, value } = await reader.read();
      if (done) break;
      buffer += decoder.decode(value, { stream: true });
      const parts = buffer.split('\n\n');
      buffer = parts.pop() ?? '';
      for (const part of parts) {
        let event = 'message';
        let data = '';
        for (const line of part.split('\n')) {
          if (line.startsWith('event:')) event = line.slice(6).trim();
          else if (line.startsWith('data:')) data += line.slice(5).trim();
        }
        if (data) onEvent(event, data);
      }
    }
  } finally {
    reader.releaseLock();
  }
}

export async function streamAnthropic(
  apiKey: string,
  req: LlmRequest,
  cb: LlmStreamCallbacks,
): Promise<LlmResponse> {
  const res = await fetch(ANTHROPIC_URL, {
    method: 'POST',
    headers: {
      'content-type': 'application/json',
      'x-api-key': apiKey,
      'anthropic-version': '2023-06-01',
      'anthropic-dangerous-direct-browser-access': 'true',
    },
    body: JSON.stringify({
      model: req.model,
      system: req.system,
      messages: anthropicMessages(req),
      tools: req.tools,
      max_tokens: req.max_tokens ?? 4096,
      stream: true,
    }),
    signal: cb.signal,
  });
  if (!res.ok || !res.body) {
    const text = await res.text();
    throw new Error(`anthropic ${res.status}: ${text}`);
  }

  const blocks: AssistantBlock[] = [];
  let currentText = '';
  const toolBlocks = new Map<number, { id: string; name: string; json: string }>();
  let stopReason: StopReason = 'end_turn';

  await parseSseStream(
    res.body,
    (_event, data) => {
      let j: Record<string, unknown>;
      try {
        j = JSON.parse(data) as Record<string, unknown>;
      } catch {
        return;
      }
      const type = j.type as string;

      if (type === 'content_block_start') {
        const block = j.content_block as { type?: string; id?: string; name?: string } | undefined;
        const index = j.index as number;
        if (block?.type === 'tool_use' && block.id && block.name) {
          toolBlocks.set(index, { id: block.id, name: block.name, json: '' });
        } else if (block?.type === 'text') {
          currentText = '';
        }
      } else if (type === 'content_block_delta') {
        const delta = j.delta as { type?: string; text?: string; partial_json?: string } | undefined;
        const index = j.index as number;
        if (delta?.type === 'text_delta' && delta.text) {
          currentText += delta.text;
          cb.onTextDelta(delta.text);
        } else if (delta?.type === 'input_json_delta' && delta.partial_json) {
          const tb = toolBlocks.get(index);
          if (tb) tb.json += delta.partial_json;
        }
      } else if (type === 'content_block_stop') {
        const index = j.index as number;
        if (toolBlocks.has(index)) {
          const tb = toolBlocks.get(index)!;
          let input: unknown = {};
          try {
            input = JSON.parse(tb.json || '{}');
          } catch {
            input = {};
          }
          blocks.push({ type: 'tool_use', id: tb.id, name: tb.name, input });
          toolBlocks.delete(index);
        } else if (currentText) {
          blocks.push({ type: 'text', text: currentText });
          currentText = '';
        }
      } else if (type === 'message_delta') {
        const delta = j.delta as { stop_reason?: StopReason } | undefined;
        if (delta?.stop_reason) stopReason = delta.stop_reason;
      } else if (type === 'message_stop') {
        if (currentText) {
          blocks.push({ type: 'text', text: currentText });
          currentText = '';
        }
      }
    },
    cb.signal,
  );

  if (currentText) blocks.push({ type: 'text', text: currentText });

  return {
    content: blocks,
    stop_reason: blocks.some((b) => b.type === 'tool_use') ? 'tool_use' : stopReason,
  };
}

/* OpenAI-compatible Chat Completions API with tool_calls (OpenAI + OpenRouter) */
const OPENAI_URL = 'https://api.openai.com/v1/chat/completions';
const OPENROUTER_URL = 'https://openrouter.ai/api/v1/chat/completions';

interface OpenAICompatibleOptions {
  url: string;
  providerLabel: string;
  extraHeaders?: Record<string, string>;
}

function buildOpenAiMessages(req: LlmRequest) {
  const messages: Array<{ role: string; content?: string; tool_calls?: unknown[]; tool_call_id?: string }> = [];
  messages.push({ role: 'system', content: req.system });
  for (const m of req.messages) {
    if (m.role === 'system') continue;
    if (m.role === 'user') {
      if (typeof m.content === 'string') {
        messages.push({ role: 'user', content: m.content });
      } else {
        for (const part of m.content) {
          if (part.type === 'tool_result') {
            messages.push({
              role: 'tool',
              tool_call_id: part.tool_use_id,
              content: part.content,
            });
          }
        }
      }
    } else if (m.role === 'assistant') {
      const blocks = assistantBlocks(m.content);
      const text = blocks
        .filter((b): b is TextBlock => b.type === 'text')
        .map((b) => b.text)
        .join('\n');
      const toolCalls = blocks
        .filter((b): b is ToolUseBlock => b.type === 'tool_use')
        .map((b) => ({
          id: b.id,
          type: 'function',
          function: { name: b.name, arguments: JSON.stringify(b.input) },
        }));
      messages.push({ role: 'assistant', content: text || undefined, tool_calls: toolCalls });
    }
  }
  return messages;
}

async function callOpenAICompatible(
  apiKey: string,
  req: LlmRequest,
  { url, providerLabel, extraHeaders }: OpenAICompatibleOptions,
): Promise<LlmResponse> {
  const messages = buildOpenAiMessages(req);
  const tools = req.tools.map((t) => ({
    type: 'function',
    function: { name: t.name, description: t.description, parameters: t.input_schema },
  }));
  const res = await fetch(url, {
    method: 'POST',
    headers: {
      'content-type': 'application/json',
      authorization: `Bearer ${apiKey}`,
      ...extraHeaders,
    },
    body: JSON.stringify({
      model: req.model,
      messages,
      tools,
      max_tokens: req.max_tokens ?? 4096,
    }),
  });
  if (!res.ok) {
    const text = await res.text();
    throw new Error(`${providerLabel} ${res.status}: ${text}`);
  }
  const j = await res.json();
  const msg = j.choices?.[0]?.message ?? {};
  const blocks: AssistantBlock[] = [];
  const text = openAiMessageText(msg.content);
  if (text) blocks.push({ type: 'text', text });
  if (Array.isArray(msg.tool_calls)) {
    for (const tc of msg.tool_calls) {
      let input: unknown = {};
      try {
        input = JSON.parse(tc.function.arguments);
      } catch {
        input = {};
      }
      blocks.push({ type: 'tool_use', id: tc.id, name: tc.function.name, input });
    }
  }
  return {
    content: blocks,
    stop_reason: blocks.some((b) => b.type === 'tool_use') ? 'tool_use' : 'end_turn',
    usage: j.usage,
  };
}

async function streamOpenAICompatible(
  apiKey: string,
  req: LlmRequest,
  { url, providerLabel, extraHeaders }: OpenAICompatibleOptions,
  cb: LlmStreamCallbacks,
): Promise<LlmResponse> {
  const messages = buildOpenAiMessages(req);
  const tools = req.tools.map((t) => ({
    type: 'function',
    function: { name: t.name, description: t.description, parameters: t.input_schema },
  }));
  const res = await fetch(url, {
    method: 'POST',
    headers: {
      'content-type': 'application/json',
      authorization: `Bearer ${apiKey}`,
      ...extraHeaders,
    },
    body: JSON.stringify({
      model: req.model,
      messages,
      tools,
      max_tokens: req.max_tokens ?? 4096,
      stream: true,
    }),
    signal: cb.signal,
  });
  if (!res.ok || !res.body) {
    const text = await res.text();
    throw new Error(`${providerLabel} ${res.status}: ${text}`);
  }

  let text = '';
  const toolAcc = new Map<number, { id: string; name: string; args: string }>();

  const reader = res.body.getReader();
  const decoder = new TextDecoder();
  let buffer = '';

  try {
    while (true) {
      if (cb.signal?.aborted) throw new DOMException('Aborted', 'AbortError');
      const { done, value } = await reader.read();
      if (done) break;
      buffer += decoder.decode(value, { stream: true });
      const lines = buffer.split('\n');
      buffer = lines.pop() ?? '';
      for (const line of lines) {
        const trimmed = line.trim();
        if (!trimmed.startsWith('data:')) continue;
        const data = trimmed.slice(5).trim();
        if (data === '[DONE]') continue;
        let j: { choices?: Array<{ delta?: { content?: string; tool_calls?: Array<{ index: number; id?: string; function?: { name?: string; arguments?: string } }> } }> };
        try {
          j = JSON.parse(data);
        } catch {
          continue;
        }
        const delta = j.choices?.[0]?.delta;
        if (!delta) continue;
        if (delta.content) {
          text += delta.content;
          cb.onTextDelta(delta.content);
        }
        if (Array.isArray(delta.tool_calls)) {
          for (const tc of delta.tool_calls) {
            const idx = tc.index ?? 0;
            if (!toolAcc.has(idx)) {
              toolAcc.set(idx, { id: tc.id ?? `call_${idx}`, name: tc.function?.name ?? '', args: '' });
            }
            const acc = toolAcc.get(idx)!;
            if (tc.id) acc.id = tc.id;
            if (tc.function?.name) acc.name = tc.function.name;
            if (tc.function?.arguments) acc.args += tc.function.arguments;
          }
        }
      }
    }
  } finally {
    reader.releaseLock();
  }

  const blocks: AssistantBlock[] = [];
  if (text) blocks.push({ type: 'text', text });
  for (const tc of toolAcc.values()) {
    let input: unknown = {};
    try {
      input = JSON.parse(tc.args || '{}');
    } catch {
      input = {};
    }
    blocks.push({ type: 'tool_use', id: tc.id, name: tc.name, input });
  }
  return {
    content: blocks,
    stop_reason: blocks.some((b) => b.type === 'tool_use') ? 'tool_use' : 'end_turn',
  };
}

export async function callOpenAI(apiKey: string, req: LlmRequest): Promise<LlmResponse> {
  return callOpenAICompatible(apiKey, req, { url: OPENAI_URL, providerLabel: 'openai' });
}

export async function callOpenRouter(apiKey: string, req: LlmRequest): Promise<LlmResponse> {
  return callOpenAICompatible(apiKey, req, {
    url: OPENROUTER_URL,
    providerLabel: 'openrouter',
    extraHeaders: {
      'HTTP-Referer': 'https://github.com/tmux-puppet-master',
      'X-Title': 'Puppet Master',
    },
  });
}

export async function callLlm(
  provider: LlmProvider,
  apiKey: string,
  req: LlmRequest,
): Promise<LlmResponse> {
  switch (provider) {
    case 'anthropic':
      return callAnthropic(apiKey, req);
    case 'openai':
      return callOpenAI(apiKey, req);
    case 'openrouter':
      return callOpenRouter(apiKey, req);
    default: {
      const _exhaustive: never = provider;
      throw new Error(`unsupported provider: ${String(_exhaustive)}`);
    }
  }
}

/** Stream tokens to callbacks; returns the assembled response for tool routing. */
export async function streamLlm(
  provider: LlmProvider,
  apiKey: string,
  req: LlmRequest,
  cb: LlmStreamCallbacks,
): Promise<LlmResponse> {
  switch (provider) {
    case 'anthropic':
      return streamAnthropic(apiKey, req, cb);
    case 'openai':
      return streamOpenAICompatible(apiKey, req, { url: OPENAI_URL, providerLabel: 'openai' }, cb);
    case 'openrouter':
      return streamOpenAICompatible(
        apiKey,
        req,
        {
          url: OPENROUTER_URL,
          providerLabel: 'openrouter',
          extraHeaders: {
            'HTTP-Referer': 'https://github.com/tmux-puppet-master',
            'X-Title': 'Puppet Master',
          },
        },
        cb,
      );
    default: {
      const _exhaustive: never = provider;
      throw new Error(`unsupported provider: ${String(_exhaustive)}`);
    }
  }
}
