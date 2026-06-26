<script lang="ts">
  // Onboarding 引导窗口。2026-06-22 从 App.svelte 拆出（阶段 2-4）。
  // 纯展示组件：步骤数据与回调由 App.svelte 通过 props 注入，自身不持有业务 state。
  import type { OnboardingStep } from "./lib/types";

  let {
    steps,
    step,
    doNotAutoShow,
    iconUrl,
    onPrev,
    onNext,
    onSelectStep,
    onToggleAutoShow
  }: {
    steps: OnboardingStep[];
    step: number;
    doNotAutoShow: boolean;
    iconUrl: string;
    onPrev: () => void;
    onNext: () => void;
    onSelectStep: (index: number) => void;
    onToggleAutoShow: (checked: boolean) => void;
  } = $props();

  const current = $derived(steps[Math.min(step, steps.length - 1)] ?? steps[0]);
</script>

<div class={`onboarding-app visual-${current.visual}`}>
  <main class="onboarding-stage">
    <div class="onboarding-visual" aria-hidden="true">
      {#if current.visual === "welcome"}
        <div class="welcome-visual">
          <img class="looppulse-icon-large" src={iconUrl} alt="" />
          <div class="welcome-signal-grid">
            {#each ["ok", "work", "warning", "critical", "idle", "idle", "ok", "work", "idle"] as tone}
              <span class={`mini-signal tone-${tone}`}></span>
            {/each}
          </div>
        </div>
      {:else if current.visual === "menubar"}
        <div class="menubar-visual">
          <div class="mock-display">
            <div class="mock-menubar">
              <span class="apple-dot"></span>
              <div></div>
              <span class="mock-status-icon active"><img src={iconUrl} alt="" /></span>
              <span class="mock-status-icon"></span>
              <span class="mock-status-icon small"></span>
            </div>
            <div class="mock-panel">
              <div class="mock-panel-head">
                <strong>LoopPulse</strong>
                <span></span>
              </div>
              <div class="mock-session-card warning">
                <i></i>
                <div><strong>mobile-app</strong><span>Claude Code · 待确认</span></div>
                <button>聚焦</button>
              </div>
              <div class="mock-session-card ok">
                <i></i>
                <div><strong>api-server</strong><span>Codex · 待命</span></div>
                <button>聚焦</button>
              </div>
            </div>
          </div>
        </div>
      {:else if current.visual === "signals"}
        <div class="signals-visual">
          <div class="signal-demo-card tone-ok"><i></i><strong>存活正常</strong><span>Agent 可识别且无告警</span></div>
          <div class="signal-demo-card tone-work"><i></i><strong>工作中</strong><span>正在思考或调用工具</span></div>
          <div class="signal-demo-card tone-warning"><i></i><strong>待确认</strong><span>等待你确认或注意</span></div>
          <div class="signal-demo-card tone-critical"><i></i><strong>需要处理</strong><span>错误、限流或假死</span></div>
        </div>
      {:else if current.visual === "alerts"}
        <div class="alerts-visual">
          <div class="notification-mock">
            <div class="notification-icon"><img src={iconUrl} alt="" /></div>
            <div>
              <strong>LoopPulse</strong>
              <span>Codex 可能已停在等待确认</span>
              <em>点击通知后可直接定位到会话详情</em>
            </div>
          </div>
          <div class="focus-mock-card">
            <div>
              <span>告警原因</span>
              <strong>等待用户决策</strong>
              <p>Agent 已停止执行，正在等待你批准或选择下一步。</p>
            </div>
            <button>聚焦窗口</button>
          </div>
        </div>
      {:else}
        <div class="privacy-visual">
          <div class="privacy-shield">
            <span>隐</span>
          </div>
          <div class="privacy-field-grid">
            {#each ["身份", "状态", "风险", "用量", "上下文", "路径", "环境", "时间线"] as field}
              <span>{field}</span>
            {/each}
          </div>
          <div class="privacy-redacted">
            <span></span><span></span><span></span>
          </div>
        </div>
      {/if}
    </div>

    <div class="onboarding-copy">
      <span>{current.eyebrow}</span>
      <h1>{current.title}</h1>
      <p>{current.summary}</p>
      <div class="onboarding-body">
        {#each current.body as paragraph}
          <p>{paragraph}</p>
        {/each}
      </div>
    </div>
  </main>

  <footer class="onboarding-footer">
    <label class="onboarding-check">
      <input
        type="checkbox"
        checked={doNotAutoShow}
        onchange={(event) => onToggleAutoShow((event.currentTarget as HTMLInputElement).checked)}
      />
      以后不再自动显示
    </label>
    <div class="onboarding-actions">
      <button class="onboarding-nav-btn" disabled={step === 0} onclick={onPrev}>上一步</button>
      <div class="onboarding-progress" aria-label={`第 ${step + 1} 步，共 ${steps.length} 步`}>
        {#each steps as s, index}
          <button
            class:active={index === step}
            aria-label={s.title}
            onclick={() => onSelectStep(index)}
          ></button>
        {/each}
      </div>
      <button class="onboarding-nav-btn primary" onclick={onNext}>
        {step === steps.length - 1 ? "完成" : "下一步"}
      </button>
    </div>
  </footer>
</div>

<style>
  .onboarding-app {
    width: 100vw;
    height: 100vh;
    overflow: hidden;
    display: grid;
    grid-template-rows: minmax(0, 1fr) 62px;
    background: #242628;
    color: rgba(245, 247, 249, 0.92);
  }

  .onboarding-stage {
    min-height: 0;
    display: grid;
    grid-template-rows: 275px minmax(0, 1fr);
    padding: 34px 54px 28px;
    background:
      linear-gradient(180deg, rgba(255, 255, 255, 0.018), transparent 38%),
      #242628;
  }

  .onboarding-visual {
    position: relative;
    display: flex;
    align-items: center;
    justify-content: center;
    border-bottom: 1px solid rgba(255, 255, 255, 0.10);
  }

  .onboarding-copy {
    width: min(760px, 100%);
    justify-self: center;
    text-align: center;
    padding-top: 28px;
  }

  .onboarding-copy > span {
    display: block;
    color: rgba(78, 202, 255, 0.86);
    font-size: 12px;
    font-weight: 700;
  }

  .onboarding-copy h1 {
    margin: 9px 0 0;
    font-size: 35px;
    line-height: 1.12;
    letter-spacing: 0;
    color: rgba(255, 255, 255, 0.88);
  }

  .onboarding-copy > p {
    margin: 12px auto 0;
    max-width: 640px;
    color: rgba(255, 255, 255, 0.72);
    font-size: 16px;
    line-height: 1.48;
    font-weight: 650;
  }

  .onboarding-body {
    display: flex;
    flex-direction: column;
    align-items: center;
    gap: 7px;
    margin-top: 13px;
    text-align: center;
  }

  .onboarding-body p {
    margin: 0;
    max-width: 640px;
    color: rgba(255, 255, 255, 0.68);
    font-size: 16px;
    line-height: 1.48;
    font-weight: 650;
  }

  .onboarding-footer {
    display: grid;
    grid-template-columns: 1fr auto 1fr;
    align-items: center;
    gap: 16px;
    padding: 0 34px;
    background: rgba(255, 255, 255, 0.135);
    border-top: 1px solid rgba(255, 255, 255, 0.10);
    backdrop-filter: blur(18px);
  }

  .onboarding-check {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    color: rgba(255, 255, 255, 0.70);
    font-size: 13px;
    font-weight: 650;
    cursor: pointer;
    user-select: none;
  }

  .onboarding-check input {
    width: 15px;
    height: 15px;
    accent-color: rgba(78, 202, 255, 0.82);
    cursor: pointer;
  }

  .onboarding-progress {
    display: inline-flex;
    align-items: center;
    gap: 8px;
    min-width: 82px;
    justify-content: center;
  }

  .onboarding-progress button {
    appearance: none;
    width: 6px;
    height: 6px;
    padding: 0;
    border: 0;
    border-radius: 50%;
    background: rgba(255, 255, 255, 0.24);
    cursor: pointer;
  }

  .onboarding-progress button.active {
    width: 20px;
    border-radius: 999px;
    background: rgba(78, 202, 255, 0.82);
    box-shadow: 0 0 12px rgba(78, 202, 255, 0.22);
  }

  .onboarding-actions {
    justify-self: center;
    grid-column: 2;
    display: inline-flex;
    align-items: center;
    gap: 14px;
  }

  .onboarding-actions .onboarding-nav-btn {
    appearance: none;
    min-width: 76px;
    height: 30px;
    border-radius: 8px;
    border: 1px solid rgba(255, 255, 255, 0.14);
    background: rgba(255, 255, 255, 0.12);
    color: rgba(255, 255, 255, 0.82);
    font: inherit;
    font-size: 13px;
    font-weight: 700;
    cursor: pointer;
  }

  .onboarding-actions .onboarding-nav-btn:disabled {
    opacity: 0.34;
    cursor: default;
  }

  .onboarding-actions .onboarding-nav-btn.primary {
    border-color: rgba(78, 202, 255, 0.30);
    background: rgba(78, 202, 255, 0.22);
    color: #fff;
  }

	  .onboarding-actions .onboarding-nav-btn:not(:disabled):hover,
	  .onboarding-progress button:hover {
	    filter: brightness(1.08);
	  }

  .welcome-visual,
  .menubar-visual,
  .signals-visual,
  .alerts-visual,
  .privacy-visual {
    width: min(760px, 100%);
    height: 230px;
    display: flex;
    align-items: center;
    justify-content: center;
  }

  .welcome-visual {
    gap: 34px;
  }

  .looppulse-icon-large {
    width: 150px;
    height: 150px;
    border-radius: 34px;
    display: block;
    object-fit: contain;
    box-shadow: 0 28px 60px rgba(0, 0, 0, 0.34);
  }

  .welcome-signal-grid {
    width: 154px;
    display: grid;
    grid-template-columns: repeat(3, 34px);
    gap: 14px;
  }

  .mini-signal {
    width: 34px;
    height: 34px;
    border-radius: 8px;
    background: rgba(255, 255, 255, 0.08);
    border: 1px solid rgba(255, 255, 255, 0.08);
  }

  .mini-signal.tone-ok {
    background: rgba(76, 212, 160, 0.78);
    box-shadow: 0 0 20px rgba(76, 212, 160, 0.30);
  }

  .mini-signal.tone-work {
    background: rgba(255, 154, 60, 0.82);
    box-shadow: 0 0 20px rgba(255, 154, 60, 0.30);
  }

  .mini-signal.tone-warning {
    background: rgba(255, 184, 77, 0.82);
    box-shadow: 0 0 20px rgba(255, 184, 77, 0.30);
  }

  .mini-signal.tone-critical {
    background: rgba(255, 92, 122, 0.86);
    box-shadow: 0 0 20px rgba(255, 92, 122, 0.30);
  }

  .mock-display {
    width: 610px;
    height: 205px;
    border-radius: 18px 18px 0 0;
    border: 2px solid rgba(255, 255, 255, 0.10);
    background: linear-gradient(180deg, rgba(255, 255, 255, 0.10), rgba(255, 255, 255, 0.035));
    overflow: hidden;
  }

  .mock-menubar {
    height: 30px;
    display: grid;
    grid-template-columns: 28px 1fr repeat(3, 28px);
    align-items: center;
    gap: 8px;
    padding: 0 14px;
    background: rgba(8, 10, 12, 0.62);
  }

  .mock-menubar div {
    min-width: 0;
  }

  .apple-dot,
  .mock-status-icon {
    display: block;
    width: 18px;
    height: 18px;
    border-radius: 6px;
    background: rgba(255, 255, 255, 0.42);
  }

  .apple-dot {
    border-radius: 50%;
  }

  .mock-status-icon {
    display: grid;
    place-items: center;
    color: rgba(255, 255, 255, 0.86);
    font-size: 10px;
    font-weight: 800;
  }

  .mock-status-icon.active {
    width: 28px;
    height: 24px;
    border-radius: 7px;
    background: rgba(255, 255, 255, 0.15);
    box-shadow: inset 0 0 0 1px rgba(255, 255, 255, 0.10);
  }

  .mock-status-icon img {
    width: 18px;
    height: 18px;
    display: block;
    object-fit: contain;
  }

  .mock-status-icon.small {
    width: 22px;
  }

  .mock-panel {
    width: 318px;
    margin: 16px 18px 0 auto;
    border-radius: 14px;
    border: 1px solid rgba(255, 255, 255, 0.13);
    background: rgba(20, 25, 31, 0.94);
    padding: 13px;
    box-shadow: 0 22px 60px rgba(0, 0, 0, 0.38);
  }

  .mock-panel-head {
    display: flex;
    align-items: center;
    justify-content: space-between;
    margin-bottom: 9px;
  }

  .mock-panel-head strong {
    font-size: 13px;
  }

  .mock-panel-head span {
    width: 58px;
    height: 18px;
    border-radius: 999px;
    background: rgba(76, 212, 160, 0.16);
  }

  .mock-session-card {
    height: 44px;
    display: grid;
    grid-template-columns: 9px 1fr 46px;
    align-items: center;
    gap: 8px;
    margin-top: 7px;
    border-radius: 9px;
    padding: 0 9px;
    background: rgba(255, 255, 255, 0.07);
  }

  .mock-session-card i {
    width: 8px;
    height: 8px;
    border-radius: 50%;
  }

  .mock-session-card.ok i {
    background: var(--obs-status-ok);
  }

  .mock-session-card.warning i {
    background: var(--obs-status-warning);
  }

  .mock-session-card strong,
  .mock-session-card span {
    display: block;
  }

  .mock-session-card strong {
    font-size: 11px;
  }

  .mock-session-card span {
    margin-top: 2px;
    color: rgba(255, 255, 255, 0.46);
    font-size: 9px;
  }

  .mock-session-card button,
  .focus-mock-card button {
    appearance: none;
    height: 24px;
    border: 0;
    border-radius: 7px;
    background: rgba(78, 202, 255, 0.20);
    color: rgba(255, 255, 255, 0.84);
    font: inherit;
    font-size: 10px;
    font-weight: 700;
  }

  .signals-visual {
    display: grid;
    grid-template-columns: repeat(4, minmax(0, 1fr));
    gap: 12px;
  }

  .signal-demo-card {
    height: 154px;
    border-radius: 14px;
    border: 1px solid rgba(255, 255, 255, 0.10);
    background: rgba(255, 255, 255, 0.06);
    padding: 20px 14px;
    text-align: center;
  }

  .signal-demo-card i {
    display: block;
    width: 34px;
    height: 34px;
    margin: 0 auto 17px;
    border-radius: 50%;
  }

  .signal-demo-card strong,
  .signal-demo-card span {
    display: block;
  }

  .signal-demo-card strong {
    font-size: 15px;
    color: rgba(255, 255, 255, 0.88);
  }

  .signal-demo-card span {
    margin-top: 9px;
    color: rgba(255, 255, 255, 0.50);
    font-size: 11px;
    line-height: 1.35;
  }

  .signal-demo-card.tone-ok i { background: var(--obs-status-ok); box-shadow: 0 0 24px rgba(76, 212, 160, 0.36); }
  .signal-demo-card.tone-work i { background: var(--obs-status-work); box-shadow: 0 0 24px rgba(255, 154, 60, 0.36); }
  .signal-demo-card.tone-warning i { background: var(--obs-status-warning); box-shadow: 0 0 24px rgba(255, 184, 77, 0.36); }
  .signal-demo-card.tone-critical i { background: var(--obs-status-critical); box-shadow: 0 0 24px rgba(255, 92, 122, 0.36); }

  .alerts-visual {
    flex-direction: column;
    gap: 18px;
  }

  .notification-mock,
  .focus-mock-card {
    width: 560px;
    border-radius: 15px;
    border: 1px solid rgba(255, 255, 255, 0.12);
    background: rgba(255, 255, 255, 0.075);
    box-shadow: 0 20px 44px rgba(0, 0, 0, 0.22);
  }

  .notification-mock {
    min-height: 78px;
    display: grid;
    grid-template-columns: 42px 1fr;
    gap: 13px;
    align-items: center;
    padding: 14px 16px;
  }

  .notification-icon {
    width: 42px;
    height: 42px;
    display: grid;
    place-items: center;
    border-radius: 10px;
    background: #121920;
    color: #fff;
    font-weight: 850;
  }

  .notification-icon img {
    width: 32px;
    height: 32px;
    display: block;
    object-fit: contain;
  }

  .notification-mock strong,
  .notification-mock span,
  .notification-mock em {
    display: block;
  }

  .notification-mock strong {
    font-size: 12px;
  }

  .notification-mock span {
    margin-top: 4px;
    font-size: 13px;
    color: rgba(255, 255, 255, 0.82);
  }

  .notification-mock em {
    margin-top: 4px;
    font-style: normal;
    font-size: 10px;
    color: rgba(255, 255, 255, 0.42);
  }

  .focus-mock-card {
    min-height: 96px;
    display: grid;
    grid-template-columns: 1fr auto;
    align-items: center;
    gap: 18px;
    padding: 16px;
  }

  .focus-mock-card span,
  .focus-mock-card strong,
  .focus-mock-card p {
    display: block;
  }

  .focus-mock-card span {
    color: var(--obs-status-warning);
    font-size: 11px;
  }

  .focus-mock-card strong {
    margin-top: 5px;
    font-size: 18px;
  }

  .focus-mock-card p {
    margin: 6px 0 0;
    color: rgba(255, 255, 255, 0.50);
    font-size: 12px;
  }

  .focus-mock-card button {
    width: 88px;
    height: 32px;
  }

  .privacy-visual {
    flex-direction: column;
    gap: 16px;
  }

  .privacy-shield {
    width: 78px;
    height: 88px;
    display: grid;
    place-items: center;
    border-radius: 26px 26px 34px 34px;
    background:
      linear-gradient(145deg, rgba(76, 212, 160, 0.28), rgba(78, 202, 255, 0.18)),
      rgba(255, 255, 255, 0.06);
    border: 1px solid rgba(76, 212, 160, 0.28);
    box-shadow: 0 22px 50px rgba(76, 212, 160, 0.16);
    color: rgba(255, 255, 255, 0.90);
    font-size: 29px;
    font-weight: 850;
  }

  .privacy-field-grid {
    display: grid;
    grid-template-columns: repeat(4, auto);
    gap: 8px;
  }

  .privacy-field-grid span {
    min-width: 68px;
    height: 28px;
    display: grid;
    place-items: center;
    border-radius: 999px;
    background: rgba(78, 202, 255, 0.13);
    border: 1px solid rgba(78, 202, 255, 0.24);
    color: rgba(255, 255, 255, 0.78);
    font-size: 11px;
    font-weight: 700;
  }

  .privacy-redacted {
    width: 410px;
    display: grid;
    gap: 8px;
    padding: 14px;
    border-radius: 12px;
    background: rgba(0, 0, 0, 0.18);
    border: 1px solid rgba(255, 255, 255, 0.08);
  }

  .privacy-redacted span {
    height: 10px;
    border-radius: 999px;
    background: rgba(255, 255, 255, 0.12);
  }

  .privacy-redacted span:nth-child(2) { width: 72%; }
  .privacy-redacted span:nth-child(3) { width: 48%; }
</style>
