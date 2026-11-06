// The custom rules text editor (UI-LEFT B2, components/RuleTextEditor.tsx): Board Setup > Custom Rules' Syntax help window (PANEL_SETUP_RULES::OnSyntaxHelp:
// KiCad's help texts as its markdown converter renders them, in a modeless "Syntax Help" window the link brings back, with HTML_MESSAGE_BOX's search),
// the completion of `onScintillaCharAdded` as the text is typed (the constraint types after `(constraint `, the list narrowed by the word, arrows and
// Enter, the quote after `(condition `, the properties after `A.`, the board's net classes after `NetClass == '`, Ctrl+Space, Escape closing the list),
// the brace light, Ctrl+/, the error link's caret jump, the right-click menu, Escape's "Cancel Changes?" (No keeps the text, Yes puts the board's back,
// and then Escape is the dialog's), and the DRC Rule Editor's Custom rule (its own completion) and condition Syntax help. Nothing is applied: the
// board's rules text is compared with how it was at the end.
// Characters are typed with `document.execCommand( "insertText" )` (an input event like a key's), keys sent as keydown events to the text box.
// Run it in the page of a served board made from examples/mcu_board_30plus.yaml (`eda board new examples/mcu_board_30plus.yaml -o <dir>` then
// `eda board serve -C <dir> --port N --ui web/studio/dist`) with `PLAYWRIGHT_MODULE=... node web/studio/e2e/run-check.mjs http://localhost:N/
// web/studio/e2e/custom-rules-editor.check.js`.
// It answers through `window.__customRulesEditorCheck`: { results: [{ name, ok, notes }], done, ok, wait(ms) }.
// run-all-checks: board=mcu30 minutes=5
(async () => {
  const results = [];
  const check = (window.__customRulesEditorCheck = { results, done: false, ok: false, wait: null });
  const sleep = async (ms) => { const t0 = Date.now(); while (Date.now() - t0 < ms) await fetch("/api/version").then((r) => r.text()); };
  check.wait = async (ms = 30000) => { const t0 = Date.now(); while (!check.done && Date.now() - t0 < ms) await fetch("/index.html").then((r) => r.text()); return { done: check.done, ok: check.ok, n: results.length, failed: results.filter((r) => !r.ok).map((r) => r.name), results: check.done ? results : results.slice(-2) }; };
  const state = () => fetch("/api/state", { cache: "no-store" }).then((r) => r.json());
  const tid = (id) => document.querySelector(`[data-testid="${id}"]`);
  const dialog = () => document.querySelector(".bs-dialog");
  const page = () => document.querySelector('section.bs-page[aria-label="Custom Rules"]');
  const area = () => page()?.querySelector('textarea[aria-label="DRC Rules"]');
  const options = (id = "custom-rules-editor-completions") => [...(tid(id)?.querySelectorAll('[role="option"]') ?? [])].map((o) => o.textContent);
  const selectedOption = (id = "custom-rules-editor-completions") => tid(id)?.querySelector('[aria-selected="true"]')?.textContent ?? null;
  const braces = (root = page()) => [...(root?.querySelectorAll("[data-brace]") ?? [])].map((b) => b.dataset.brace + ":" + b.textContent);
  const openPage = async () => {
    if (!dialog()) { await __eda.run("pcbnew.EditorControl.boardSetup"); await sleep(500); }
    [...document.querySelectorAll(".bs-nav-item")].find((b) => b.textContent.trim().startsWith("Custom Rules"))?.click();
    await sleep(300);
    if (!page() || page().hidden) throw new Error("Board Setup has no Custom Rules page on screen");
    return page();
  };
  const closeDialog = async () => {
    for (let i = 0; i < 3 && dialog(); i++) {
      [...document.querySelectorAll(".bs-dialog .dialog-footer button")].find((x) => ["Close", "Discard and close"].includes(x.textContent.trim()))?.click();
      await sleep(300);
    }
  };
  /** Types `text` at the caret of `el`, character by character, as the keyboard would. */
  const type = async (el, text) => {
    el.focus();
    for (const ch of text) {
      document.execCommand("insertText", false, ch);
      await sleep(15);
    }
    await sleep(60);
  };
  const key = async (el, k, mods = {}) => {
    el.focus();
    el.dispatchEvent(new KeyboardEvent("keydown", { key: k, bubbles: true, cancelable: true, ...mods }));
    await sleep(80);
  };
  const caretToEnd = (el) => { el.focus(); el.setSelectionRange(el.value.length, el.value.length); el.dispatchEvent(new Event("select", { bubbles: true })); };
  const setValue = (el, v) => {
    Object.getOwnPropertyDescriptor(Object.getPrototypeOf(el), "value").set.call(el, String(v));
    el.dispatchEvent(new Event("input", { bubbles: true }));
  };
  const J = (v) => JSON.stringify(v ?? null);

  const scenario = async (name, fn) => {
    const entry = { name, ok: true, notes: [] };
    const t = (cond, msg) => { entry.notes.push(`${cond ? "ok" : "FAIL"}: ${msg}`); if (!cond) entry.ok = false; return !!cond; };
    try { await fn(t); } catch (e) { entry.notes.push(`FAIL: threw ${(e && e.stack) || e}`); entry.ok = false; }
    results.push(entry);
  };

  if (__eda.state().tab !== "pcb") { [...document.querySelectorAll("button, [role=tab]")].find((e) => e.textContent.trim() === "PCB")?.click(); await sleep(900); }
  const s0 = await state();
  const original = s0.board_rules?.custom_rules_text ?? "";
  const netClasses = (s0.board_rules?.net_classes ?? []).map((c) => c.name);

  (async () => {
    await scenario("Syntax help: KiCad's help in a modeless Syntax Help window, brought back by the link, with its search", async (t) => {
      const p = await openPage();
      const link = tid("rules-syntax-help");
      t(link?.textContent.trim() === "Syntax help" && link?.title === "Show syntax help window", `the link and its tooltip (${link?.textContent} / ${link?.title})`);
      link.click();
      await sleep(300);
      const win = tid("rules-syntax-help-window");
      t(!!win && !win.hidden && win.getAttribute("aria-label") === "Syntax Help" && win.querySelector(".hmb-title span")?.textContent === "Syntax Help", "the window opens, titled Syntax Help");
      const heads = [...(win?.querySelectorAll("h3") ?? [])].map((h) => h.textContent);
      t(J(heads) === J(["Top-level Clauses", "Rule Clauses", "Constraints", "Items", "Severity Names", "Examples", "Notes", "Graphical Rule Editor", "Properties", "Expression functions", "More Examples", "Documentation"]), `KiCad's headings in its order (${heads})`);
      const cells = win?.querySelectorAll("td") ?? [];
      t(cells.length > 100 && [...cells].every((c) => c.getAttribute("valign") === "top"), `the constraints table, every cell valign=top (${cells.length})`);
      t(/Checks the width of annular rings on vias\./.test(win?.textContent ?? "") && /Use Ctrl\+\/ to comment or uncomment line\(s\)\./.test(win?.textContent ?? ""), "KiCad's words");
      t(!!dialog(), "Board Setup stays open beside it (modeless)");
      // HTML_MESSAGE_BOX: a character typed opens the search bar and marks the matches
      win.focus();
      win.dispatchEvent(new KeyboardEvent("keydown", { key: "k", bubbles: true, cancelable: true }));
      await sleep(250);
      const bar = tid("rules-syntax-help-window-search");
      t(!!bar && /^\d+\/\d+$/.test(bar.querySelector(".hmb-count")?.textContent ?? ""), `typing opens the search bar (${bar?.querySelector(".hmb-count")?.textContent})`);
      const box = bar?.querySelector("input");
      if (box) {
        setValue(box, "skew");
        box.dispatchEvent(new Event("change", { bubbles: true }));
        await sleep(200);
        const marks = win.querySelectorAll('.hmb-page span[style*="background-color"]').length;
        t(marks > 3 && /^1\/\d+$/.test(bar.querySelector(".hmb-count")?.textContent ?? ""), `"skew" is marked (${marks}, ${bar.querySelector(".hmb-count")?.textContent})`);
        box.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }));
        await sleep(200);
        t(!tid("rules-syntax-help-window-search") && !win.hidden && !!dialog(), "Escape closes the search bar only");
      }
      win.querySelector(".hmb-close")?.click();
      await sleep(200);
      t(win.hidden && !!dialog(), "the close box hides it, Board Setup stays");
      tid("rules-syntax-help").click();
      await sleep(250);
      t(!tid("rules-syntax-help-window").hidden, "a second click brings it back");
      tid("rules-syntax-help-window").focus();
      tid("rules-syntax-help-window").dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }));
      await sleep(200);
      t(tid("rules-syntax-help-window").hidden && !!dialog(), "Escape in the window closes it, not Board Setup");
      t(!!p.querySelector(".rte-gutter") && p.querySelector(".rte-gutter").textContent.startsWith("1"), "the editor has KiCad's line numbers");
    });

    await scenario("completion: (constraint offers the constraint types, the word narrows them, arrows and Enter take one; the brace light", async (t) => {
      await openPage();
      const el = area();
      caretToEnd(el);
      await type(el, "\n(rule x (constraint ");
      const all = options();
      t(all.includes("clearance") && all.includes("bridged_mask") && all.length === 33 && selectedOption() === "annular_width", `the 33 constraint types, the first selected (${all.length}: ${all.slice(0, 4)})`);
      await type(el, "cl");
      t(J(options()) === J(["clearance", "courtyard_clearance", "edge_clearance", "hole_clearance", "physical_clearance", "physical_hole_clearance", "silk_clearance"]) && selectedOption() === "clearance", `"cl": the types with cl in them, clearance selected (${options()})`);
      await key(el, "ArrowDown");
      t(selectedOption() === "courtyard_clearance", `Down moves the selection (${selectedOption()})`);
      await key(el, "ArrowUp");
      await key(el, "Enter");
      t(el.value.endsWith("(rule x (constraint clearance") && !tid("custom-rules-editor-completions"), `Enter puts clearance in for "cl" and closes the list (${J(el.value.slice(-30))})`);
      await type(el, " (min 1mm)");
      const v = el.value;
      t(J(braces()) === J(["match:(", "match:)"]), `after ")" the pair is lit (${braces()})`);
      const lit = [...page().querySelectorAll("[data-brace]")];
      // the lit "(" is the one of "(min"
      const before = (n) => { let s = ""; for (let x = n.previousSibling; x; x = x.previousSibling) s = x.textContent + s; return s; };
      t(before(lit[0]).endsWith("(rule x (constraint clearance ") && v.endsWith("(min 1mm)"), "the ( it matches is the one of (min");
      await type(el, "))");
      t(J(braces()) === J(["match:(", "match:)"]) && before(page().querySelector("[data-brace]")).endsWith("\n"), `the rule's ) lights the rule's ( (${braces()})`);
      await type(el, ")");
      t(J(braces()) === J(["bad:)"]), `a ) too many is a bad brace (${braces()})`);
      t(getComputedStyle(page().querySelector(".rte-brace-bad")).color === "rgb(255, 0, 0)", "in red");
    });

    await scenario("completion: the quote after (condition, the properties after A., the board's net classes, Ctrl+Space, Escape", async (t) => {
      await openPage();
      const el = area();
      caretToEnd(el);
      await type(el, "\n(rule y (condition ");
      t(el.value.endsWith('(condition "'), `the quote is put after (condition (${J(el.value.slice(-14))})`);
      await type(el, "A.");
      const props = options();
      t(props.includes("NetClass") && props.includes("Via_Type") && props.includes("hasNetclass('x')") && !props.some((o) => o.includes("DEPRECATED")) && !props.includes("Net_Class"), `the properties and functions (${props.length})`);
      await type(el, "NetC");
      t(selectedOption() === "NetClass" && options().includes("hasNetclass('x')"), `"NetC": NetClass selected (${options()})`);
      await key(el, "Tab");
      t(el.value.endsWith('"A.NetClass'), `Tab takes it (${J(el.value.slice(-12))})`);
      await type(el, " == '");
      if (netClasses.length) t(J(options()) === J([...netClasses].sort((a, b) => a.toLowerCase().localeCompare(b.toLowerCase()))), `the board's net classes (${options()} / ${netClasses})`);
      else t(!tid("custom-rules-editor-completions"), "the board has no net class: nothing offered");
      await type(el, "x'\")\n(rule z (");
      t(J(options()) === J(["condition", "constraint", "layer", "severity"]), `( in a rule: its clauses (${options()})`);
      // Escape closes the list only, and the next letters do not bring it back (m_suppressAutocomplete)
      await key(el, "Escape");
      t(!tid("custom-rules-editor-completions") && !!dialog() && !tid("rules-cancel-changes"), "Escape closes the list only");
      await type(el, "co");
      t(!tid("custom-rules-editor-completions"), "the letters after it offer nothing");
      // Ctrl+Space offers again where the caret is
      await key(el, " ", { ctrlKey: true });
      t(J(options()) === J(["condition", "constraint"]) && selectedOption() === "condition", `Ctrl+Space offers (${options()})`);
      await key(el, "ArrowLeft");
      t(!tid("custom-rules-editor-completions"), "another key closes the list");
      // Ctrl+/ comments the line
      const lineStart = el.value.lastIndexOf("\n") + 1;
      await key(el, "/", { ctrlKey: !/Mac/.test(navigator.platform), metaKey: /Mac/.test(navigator.platform) });
      t(el.value.slice(lineStart).startsWith("#(rule z (co"), `Ctrl+/ comments the line (${J(el.value.slice(lineStart))})`);
    });

    await scenario("the error link puts the caret on its line; the right-click menu", async (t) => {
      const p = await openPage();
      const el = area();
      const before = el.value;
      setValue(el, "(version 1)\n(rule a (constraint nonsense (min 1mm)))\n");
      await sleep(200);
      [...p.closest(".dialog").querySelectorAll("button")].find((b) => b.textContent.trim() === "Check rule syntax")?.click();
      let link = null;
      for (let i = 0; i < 40 && !link; i++) { await sleep(150); link = document.querySelector("[data-rules-report] .bs-link"); }
      t(!!link, `the report has a link (${document.querySelector("[data-rules-report]")?.textContent})`);
      if (link) {
        link.click();
        await sleep(150);
        t(document.activeElement === el && el.value.slice(0, el.selectionStart).split("\n").length === 2, `the caret goes to line 2 (${el.selectionStart})`);
      }
      el.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: 600, clientY: 400 }));
      await sleep(200);
      const menu = [...document.querySelectorAll(".menubar-dropdown .menu-node-item")].map((m) => m.textContent.trim());
      t(J(menu) === J(["Undo", "Redo", "Cut", "Copy", "Paste", "Delete", "Select All", "Zoom In", "Zoom Out"]), `OnContextMenu's entries (${menu})`);
      const size0 = parseFloat(getComputedStyle(el).fontSize);
      [...document.querySelectorAll(".menubar-dropdown .menu-node-item")].find((m) => m.textContent.trim() === "Zoom In")?.click();
      await sleep(150);
      const size1 = parseFloat(getComputedStyle(el).fontSize);
      t(size1 > size0, `Zoom In makes the text bigger (${size0} -> ${size1})`);
      el.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: 600, clientY: 400 }));
      await sleep(150);
      [...document.querySelectorAll(".menubar-dropdown .menu-node-item")].find((m) => m.textContent.trim() === "Zoom Out")?.click();
      await sleep(150);
      t(parseFloat(getComputedStyle(el).fontSize) === size0, "Zoom Out takes it back");
      el.dispatchEvent(new MouseEvent("contextmenu", { bubbles: true, cancelable: true, clientX: 600, clientY: 400 }));
      await sleep(150);
      await key(el, "Escape");
      t(!document.querySelector(".menubar-dropdown") && !!dialog(), "Escape closes the menu, not Board Setup");
      setValue(el, before);
      await sleep(100);
    });

    await scenario("Escape: Cancel Changes? -- No keeps the text, Yes puts the board's back; unchanged, Escape is the dialog's", async (t) => {
      await openPage();
      const el = area();
      t(el.value !== original, "the text was edited");
      await key(el, "Escape");
      const ask = tid("rules-cancel-changes");
      t(ask?.textContent.startsWith("Cancel Changes?") && !!dialog(), `asked, in the page (${ask?.textContent})`);
      const edited = el.value;
      [...(ask?.querySelectorAll("button") ?? [])].find((b) => b.textContent === "No")?.click();
      await sleep(150);
      t(!tid("rules-cancel-changes") && el.value === edited, "No keeps the text");
      await key(el, "Escape");
      // Escape on the question answers No
      document.activeElement.dispatchEvent(new KeyboardEvent("keydown", { key: "Escape", bubbles: true, cancelable: true }));
      await sleep(150);
      t(!tid("rules-cancel-changes") && el.value === edited && !!dialog(), "Escape on the question is No, and Board Setup stays");
      await key(el, "Escape");
      [...(tid("rules-cancel-changes")?.querySelectorAll("button") ?? [])].find((b) => b.textContent === "Yes")?.click();
      await sleep(250);
      t(el.value === original && el.selectionStart === 0 && el.selectionEnd === el.value.length, `Yes puts the board's text back, all selected (${J(el.value.slice(0, 40))})`);
      t(!/Changes not applied yet/.test(page().querySelector(".bs-msg")?.textContent ?? ""), "and the page has nothing to apply");
      await key(el, "Escape");
      await sleep(200);
      t(!dialog(), "with nothing changed, Escape closes Board Setup");
    });

    await scenario("DRC Rule Editor: the Custom rule's own completion and the brace light; Syntax help beside Conditions", async (t) => {
      await __eda.run("pcbnew.DRETool.drcRuleEditor");
      await sleep(800);
      const node = (name) => [...document.querySelectorAll('[data-testid^="re-node-"]')].find((n) => n.dataset.name === name);
      for (let i = 0; i < 6; i++) { const tw = [...document.querySelectorAll(".re-twisty")].filter((x) => x.textContent === "▸"); if (!tw.length) break; for (const x of tw) x.click(); await sleep(80); }
      node("Custom rule")?.click();
      await sleep(150);
      tid("re-add")?.click();
      await sleep(300);
      const el = tid("re-custom-text");
      t(el?.tagName === "TEXTAREA" && el.value === "   (constraint clearance (min 0.2mm))", `KiCad's example body in the editor (${J(el?.value)})`);
      t(!tid("re-syntax-help"), "no Syntax help for a Custom rule (KiCad hides it)");
      caretToEnd(el);
      await type(el, "\n   (constraint ");
      const opts = options("re-custom-text-completions");
      t(opts.includes("max_error") && !opts.includes("bridged_mask") && opts.length === 30, `DRC_RE_CUSTOM_RULE_PANEL's list (${opts.length}: max_error ${opts.includes("max_error")})`);
      await key(el, "Escape");
      t(!tid("re-custom-text-completions") && !!tid("rule-editor"), "Escape closes the list, the dialog stays");
      await type(el, "clearance (min 1mm)");
      t(J(braces(tid("re-custom-text-box"))) === J(["match:(", "match:)"]), `the brace light (${braces(tid("re-custom-text-box"))})`);
      // a rule with conditions: Syntax help beside them opens the condition help
      node("Minimum clearance")?.click();
      await sleep(150);
      tid("re-add")?.click();
      await sleep(300);
      tid("re-syntax-help")?.click();
      await sleep(250);
      const win = tid("re-syntax-help-window");
      const heads = [...(win?.querySelectorAll("h3") ?? [])].map((h) => h.textContent);
      t(!!win && !win.hidden && heads[0] === "Clause" && heads.includes("Expression functions") && !heads.includes("Constraints"), `PANEL_DRC_RULE_EDITOR's help: its texts (${heads})`);
      win?.querySelector(".hmb-close")?.click();
      await sleep(150);
      // the condition row's Custom Query is the same editor (no completion: KiCad's char-added function there does nothing)
      const type0 = tid("re-cond-type");
      if (type0) {
        Object.getOwnPropertyDescriptor(HTMLSelectElement.prototype, "value").set.call(type0, "custom");
        type0.dispatchEvent(new Event("change", { bubbles: true }));
        await sleep(200);
        const q = tid("re-cond-custom");
        t(q?.tagName === "TEXTAREA" && !!tid("re-cond-custom-box"), "Custom Query is the rules text editor");
        if (q) {
          await type(q, "A.Type == 'Via' && (A.x");
          t(!tid("re-cond-custom-completions"), "it offers nothing");
        }
      }
      // close without saving
      for (let i = 0; i < 6 && tid("rule-editor"); i++) {
        tid("re-cancel")?.click();
        await sleep(250);
        tid("re-ask-no")?.click();
        await sleep(250);
      }
      t(!tid("rule-editor"), "closed without saving");
    });

    await scenario("the board's rules are as they were", async (t) => {
      const s = await state();
      t((s.board_rules?.custom_rules_text ?? "") === original, `nothing was applied (${J((s.board_rules?.custom_rules_text ?? "").slice(0, 60))})`);
    });

    try { await closeDialog(); } catch { /* ignore */ }
    check.ok = results.every((r) => r.ok);
    check.done = true;
  })();
})();
