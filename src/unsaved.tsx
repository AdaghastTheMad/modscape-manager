import { useState } from "react";
import { Button, Modal } from "./components/kit";

/**
 * The editor that's open right now (payload or template file), so leaving it
 * (another item, another tab) can ask first. Editors set this while mounted.
 */
export const unsaved = {
  /** Asked at the moment of leaving; editors set it on every render. */
  isDirty: (): boolean => false,
  /** What's being edited, e.g. a payload name or a file path. */
  name: "",
  /** Like `name`, worked out when asked (wins over `name`). */
  nameNow: null as null | (() => string),
  /** Shown under the name: why nothing is saved yet. */
  hint: "Changes only stick when you save them.",
  save: async (): Promise<boolean> => true,
};

/** Runs `go` now if nothing is unsaved; otherwise asks Keep editing / Discard / Save. */
export function useUnsavedGuard() {
  const [pending, setPending] = useState<(() => void) | null>(null);
  const guard = (go: () => void) => { if (unsaved.isDirty()) setPending(() => go); else go(); };
  const prompt = pending && (
    <Modal title="Unsaved changes" width={460} onClose={() => setPending(null)}>
      <div className="stack-lg">
        <p><strong>{unsaved.nameNow?.() || unsaved.name || "This"}</strong> has changes that aren't saved. {unsaved.hint}</p>
        <div className="row" style={{ justifyContent: "flex-end" }}>
          <Button onClick={() => setPending(null)}>Keep editing</Button>
          <Button variant="danger" onClick={() => { unsaved.isDirty = () => false; const go = pending; setPending(null); go(); }}>Discard changes</Button>
          <Button variant="primary" onClick={async () => { if (await unsaved.save()) { const go = pending; setPending(null); go(); } else setPending(null); }}>Save</Button>
        </div>
      </div>
    </Modal>
  );
  return { guard, prompt };
}
