// "Save as file" under a chat message: the text becomes a Markdown file on the Artifacts page. The button shows on hover
// and on keyboard focus; after the save it says where the file went.
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
      <p className="ck-sub ck-artifact-saved" role="status">
        {ARTIFACT_COPY.saved(state.name)}
      </p>
    );
  }
  return (
    <>
      <button type="button" className="ck-artifact-save" disabled={state.kind === 'saving'} onClick={() => void save()}>
        {ARTIFACT_COPY.saveAsFile}
      </button>
      {state.kind === 'failed' && (
        <span className="ck-sub ck-artifact-note" role="alert">
          {state.text}
        </span>
      )}
    </>
  );
}
