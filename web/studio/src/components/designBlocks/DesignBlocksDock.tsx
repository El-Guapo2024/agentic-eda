// The Design Blocks pane as a docked pane (`DESIGN_BLOCK_PANE` in the frame's AUI manager, caption "Design Blocks", `CloseButton( true )`): the schematic's sits in a column of its
// own on the right of the sheet (`Right().Layer( 3 )`), the board editor's is the outermost pane on the right (`Right().Layer( 5 )`), next to the Appearance dock. View >
// Design Blocks (Shift+B places a block; the View menu entry shows or hides this) and the pane's close button show and hide it; whether it is shown is `state/designBlocks.ts`'s.
import { useStudioState } from "../../state/store";
import { setDesignBlockPaneShown, type BlockEditor } from "../../state/designBlocks";
import { DesignBlocksPane } from "./DesignBlocksPane";

export function DesignBlocksDock({ editor }: { editor: BlockEditor }) {
  const state = useStudioState();
  return (
    <div className="dock db-dock" data-testid="db-dock" data-editor={editor}>
      <header className="dock-panel-caption">
        <span className="dock-panel-title">Design Blocks</span>
        <button type="button" className="dock-panel-close" aria-label="Hide Design Blocks" title="Hide Design Blocks" onClick={() => setDesignBlockPaneShown(editor, false)}>
          {"×"}
        </button>
      </header>
      <DesignBlocksPane editor={editor} hasSelection={state.selection.size > 0} />
    </div>
  );
}
