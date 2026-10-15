// Plain-language names for the transcript stream tags the backend writes (`agent_runs/transcript.rs`
// callers). The raw tags are internal; people read "Answer", not `assistant_final`.
const LABELS: Record<string, string> = {
  user_task: 'You',
  user_steer: 'You',
  user: 'You',
  assistant_final: 'Answer',
  agent: 'Agent',
  result: 'Result',
  success: 'Done',
  stdout: 'Output',
  stderr: 'Errors',
  permission: 'Approval',
  interrupted: 'Stopped',
  execution_failure: 'Failed',
  delivery: 'Sending',
  delivery_trace: 'Sending details',
  diagnostic: 'Details',
  startup: 'Starting',
};

/** A short human label for a transcript stream tag; unknown tags become sentence case (`some_tag` -> "Some tag"). */
export function transcriptStreamLabel(stream: string): string {
  const known = LABELS[stream];
  if (known) return known;
  const text = stream.replace(/[_-]+/g, ' ').trim();
  return text ? text.charAt(0).toUpperCase() + text.slice(1) : 'Other';
}

/** Streams that carry what a person or the agent said; they read as prose. The rest is output and stays monospace. */
const PROSE = new Set(['user_task', 'user_steer', 'user', 'assistant_final', 'agent', 'result', 'success']);

export function isProseStream(stream: string): boolean {
  return PROSE.has(stream);
}

/** The speaker label of a transcript row: "You" and the agent's own name for its answers; other rows keep their plain label. */
export function transcriptSpeaker(stream: string, agentName: string): string {
  return stream === 'assistant_final' || stream === 'agent' ? agentName.trim() || transcriptStreamLabel(stream) : transcriptStreamLabel(stream);
}
