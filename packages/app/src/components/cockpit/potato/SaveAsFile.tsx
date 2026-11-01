// "Save as file" in a chat message's hover toolbar (after Copy, Reply and Speak): the text becomes a Markdown file on the
// Artifacts page. After the save it says where the file went.
import { useState } from 'react';
import { ARTIFACT_COPY, type ArtifactTransport } from '../../../lib/potato-artifacts';
import { plainError } from '../../../lib/potato';

export interface SaveAsFileProps {
  botId: string;
  messageId: string;
  files: ArtifactTransport;
}

type State = { kind: 'idle' } | { kind: 'saving' } | { kind: 'saved'; name: string } | { kind: 'failed'; text: string };

export function SaveAsFile({ botId, messageId, files }: SaveAsFileProps) {
  const [state, setState] = useState<State>({ kind: 'idle' });

  const save = async () => {
    setState({ kind: 'saving' });
    try {
      const saved = await files.saveMessage(botId, messageId);
      setState({ kind: 'saved', name: saved.name });
    } catch (error) {
      setState({ kind: 'failed', text: plainError(error, 'Could not save the file.') });
    }
  };

  if (state.kind === 'saved') {
    return (
      <span className="ck-artifact-saved" role="status">
        {ARTIFACT_COPY.saved(state.name)}
      </span>
    );
  }
  return (
    <>
      <button type="button" className="ck-potato-action ck-artifact-save" disabled={state.kind === 'saving'} onClick={() => void save()}>
        {ARTIFACT_COPY.saveAsFile}
      </button>
      {state.kind === 'failed' && (
        <span className="ck-artifact-note" role="alert">
          {state.text}
        </span>
      )}
    </>
  );
}
