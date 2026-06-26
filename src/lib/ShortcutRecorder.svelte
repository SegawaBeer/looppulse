<script lang="ts">
  // 全局快捷键录制组件。
  // 进出值为 tauri-plugin-global-shortcut 认识的字符串，如 "Alt+Cmd+Space"。
  // 仅支持「修饰键 + 主键」组合键；至少一个修饰键，且只有一个主键。

  interface Props {
    value: string;
    defaultValue: string;
    onChange: (next: string) => void;
  }

  let { value, defaultValue, onChange }: Props = $props();

  let recording = $state(false);
  let hint = $state("");

  // event.code → 插件主键 token
  function mainKeyFromCode(code: string): string | null {
    if (code === "Space") return "Space";
    if (/^Key[A-Z]$/.test(code)) return code.slice(3); // KeyA → A
    if (/^Digit[0-9]$/.test(code)) return code.slice(5); // Digit1 → 1
    if (/^F([1-9]|1[0-9]|2[0-4])$/.test(code)) return code; // F1..F24
    if (code === "Minus") return "Minus";
    if (code === "Equal") return "Equal";
    if (code === "BracketLeft") return "BracketLeft";
    if (code === "BracketRight") return "BracketRight";
    if (code === "Backslash") return "Backslash";
    if (code === "Semicolon") return "Semicolon";
    if (code === "Quote") return "Quote";
    if (code === "Comma") return "Comma";
    if (code === "Period") return "Period";
    if (code === "Slash") return "Slash";
    if (code === "Backquote") return "Backquote";
    return null;
  }

  const MODIFIER_SYMBOLS: Record<string, string> = {
    Ctrl: "⌃",
    Control: "⌃",
    Alt: "⌥",
    Option: "⌥",
    Shift: "⇧",
    Cmd: "⌘",
    Command: "⌘",
    Super: "⌘"
  };

  const KEY_SYMBOLS: Record<string, string> = {
    Space: "Space",
    Minus: "-",
    Equal: "=",
    BracketLeft: "[",
    BracketRight: "]",
    Backslash: "\\",
    Semicolon: ";",
    Quote: "'",
    Comma: ",",
    Period: ".",
    Slash: "/",
    Backquote: "`"
  };

  function toDisplay(spec: string): string {
    if (!spec) return "未设置";
    const parts = spec.split("+");
    return parts
      .map((part) => MODIFIER_SYMBOLS[part] ?? KEY_SYMBOLS[part] ?? part)
      .join("");
  }

  const displayValue = $derived(toDisplay(value));

  function startRecording() {
    recording = true;
    hint = "按下你想要的组合键（需含 ⌘ ⌥ ⌃ ⇧ 之一）";
  }

  function stopRecording() {
    recording = false;
    hint = "";
  }

  function handleKeydown(event: KeyboardEvent) {
    if (!recording) return;
    event.preventDefault();
    event.stopPropagation();

    if (event.key === "Escape") {
      stopRecording();
      return;
    }

    const modifiers: string[] = [];
    if (event.ctrlKey) modifiers.push("Ctrl");
    if (event.altKey) modifiers.push("Alt");
    if (event.shiftKey) modifiers.push("Shift");
    if (event.metaKey) modifiers.push("Cmd");

    // 纯修饰键的 keydown：等待主键
    if (["Meta", "Alt", "Control", "Shift"].includes(event.key)) {
      return;
    }

    const mainKey = mainKeyFromCode(event.code);
    if (!mainKey) {
      hint = "不支持该按键，请换一个主键";
      return;
    }
    if (modifiers.length === 0) {
      hint = "请至少包含一个修饰键（⌘ ⌥ ⌃ ⇧）";
      return;
    }

    const spec = [...modifiers, mainKey].join("+");
    stopRecording();
    if (spec !== value) {
      onChange(spec);
    }
  }

  function restoreDefault() {
    if (defaultValue !== value) {
      onChange(defaultValue);
    }
  }
</script>

<svelte:window on:keydown={handleKeydown} />

<div class="shortcut-recorder">
  <button
    type="button"
    class="shortcut-display"
    class:recording
    onclick={recording ? stopRecording : startRecording}
    onblur={stopRecording}
  >
    {recording ? "录制中…" : displayValue}
  </button>
  <button type="button" class="shortcut-reset" onclick={restoreDefault}>
    恢复默认
  </button>
</div>
{#if hint}
  <div class="shortcut-hint">{hint}</div>
{/if}

<style>
  .shortcut-recorder {
    display: flex;
    align-items: center;
    gap: 8px;
  }

  .shortcut-display {
    flex: 1;
    min-width: 0;
    height: 30px;
    padding: 0 12px;
    border-radius: var(--obs-control-radius);
    border: 0.5px solid var(--obs-border-soft);
    background: var(--obs-surface-card);
    color: var(--obs-text-strong);
    font: inherit;
    font-size: 12px;
    letter-spacing: 0.04em;
    cursor: pointer;
    text-align: center;
  }

  .shortcut-display.recording {
    border-color: var(--obs-status-info-border);
    background: var(--obs-status-info-soft);
    color: var(--obs-text-strong);
  }

  .shortcut-reset {
    appearance: none;
    height: 30px;
    padding: 0 10px;
    border-radius: var(--obs-control-radius);
    border: 0.5px solid var(--obs-border-soft);
    background: transparent;
    color: var(--obs-text-muted);
    font: inherit;
    font-size: 10px;
    cursor: pointer;
    white-space: nowrap;
  }

  .shortcut-reset:hover {
    color: var(--obs-text-strong);
    border-color: var(--obs-status-info-border);
  }

  .shortcut-hint {
    margin-top: 6px;
    color: var(--obs-text-muted);
    font-size: 10px;
    line-height: 1.35;
  }
</style>
