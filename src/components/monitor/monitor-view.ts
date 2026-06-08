import { LitElement, html, css, nothing } from "lit";
import { customElement, state } from "lit/decorators.js";
import type { Gpu, Sample, SystemInfo } from "../../ipc";
import {
  getGpus,
  getSettings,
  getSystemInfo,
  monitorStart,
  monitorStop,
  openTaskManager,
  subscribe,
} from "../../ipc";
import { formatBytes } from "../../format";
import "../view-page";

const HISTORY = 60; // ~1 minute at 1Hz

interface Metric {
  key: string;
  label: string;
  value: string;
  /** Recent values for the sparkline. */
  history: number[];
  /** Fixed axis max (e.g. 100 for %); omit for auto-scaling rate metrics. */
  max?: number;
  /**
   * Scale the sparkline to the window's own min..max instead of 0..max. For
   * signals that hover in a narrow band far from zero (memory is ~50% of a big
   * total), a 0..100 axis renders an almost-flat line; min-max makes the real
   * movement visible.
   */
  minMax?: boolean;
}

/**
 * System monitor (design.md, Task 19): a GPU panel plus live CPU / RAM / network
 * / disk / GPU readouts with sparklines, fed by `monitor://sample`. Owns the
 * sampler lifecycle — it runs only while this view is mounted AND the window is
 * visible + focused, so a hidden monitor costs nothing (idle CPU ≈ 0).
 */
@customElement("monitor-view")
export class MonitorView extends LitElement {
  static styles = css`
    :host {
      display: block;
    }
    .body {
      padding: 0 48px 48px;
      display: grid;
      gap: 32px;
    }
    h2 {
      margin: 0 0 16px;
      font-family: var(--font-display);
      font-weight: 300;
      font-size: 22px;
      letter-spacing: 0.1px;
      color: var(--on-surface);
    }
    .gpus {
      display: grid;
      gap: 12px;
    }
    .gpu {
      background: var(--surface);
      border: 1px solid var(--hairline);
      border-radius: var(--rounded-md);
      padding: 16px;
    }
    .gpu-name {
      font-weight: 600;
      font-size: 16px;
      color: var(--on-surface);
    }
    .sys-head {
      display: flex;
      align-items: center;
      justify-content: space-between;
      gap: 16px;
      margin: 0 0 16px;
    }
    .sys-head h2 {
      margin: 0;
    }
    .tm-btn {
      flex: none;
      font: 700 13px/1.25 var(--font-body);
      letter-spacing: 0.3px;
      padding: 8px 16px;
      border-radius: var(--rounded-full);
      border: 1px solid var(--hairline);
      background: var(--surface-elevated);
      color: var(--on-surface);
      cursor: pointer;
    }
    .tm-btn:hover {
      border-color: var(--primary);
      color: var(--primary);
    }
    .tm-btn:focus-visible {
      outline: 2px solid var(--primary);
      outline-offset: 2px;
    }
    .sysgrid {
      display: grid;
      grid-template-columns: repeat(auto-fill, minmax(170px, 1fr));
      gap: 12px;
    }
    .syscard {
      display: flex;
      align-items: center;
      gap: 12px;
      background: var(--surface);
      border: 1px solid var(--hairline);
      border-radius: var(--rounded-md);
      padding: 14px 16px;
      min-width: 0;
    }
    .syscard.wide {
      grid-column: span 2;
    }
    .syscard .ico {
      font-size: 22px;
      line-height: 1;
    }
    .sys-text {
      min-width: 0;
    }
    .sys-label {
      font-size: 11px;
      letter-spacing: 0.4px;
      text-transform: uppercase;
      color: var(--on-surface-muted);
    }
    .sys-val {
      font-size: 15px;
      font-weight: 600;
      color: var(--on-surface);
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
    }
    @media (max-width: 480px) {
      .syscard.wide {
        grid-column: span 1;
      }
    }
    .muted {
      font-size: 14px;
      color: var(--on-surface-muted);
    }
    .metrics {
      display: grid;
      grid-template-columns: repeat(auto-fill, minmax(220px, 1fr));
      gap: 16px;
    }
    .metric {
      background: var(--surface);
      border: 1px solid var(--hairline);
      border-radius: var(--rounded-md);
      padding: 16px;
    }
    .metric-label {
      font-size: 14px;
      color: var(--on-surface-muted);
    }
    .metric-value {
      margin: 4px 0 10px;
      font-family: var(--font-display);
      font-weight: 300;
      font-size: 28px;
      line-height: 1.1;
      color: var(--on-surface);
    }
    svg.spark {
      display: block;
      width: 100%;
      height: 32px;
    }
    svg.spark path {
      fill: none;
      stroke: var(--primary);
      stroke-width: 2;
      stroke-linejoin: round;
      stroke-linecap: round;
      vector-effect: non-scaling-stroke;
    }
    @media (max-width: 768px) {
      .body {
        padding-left: 24px;
        padding-right: 24px;
      }
    }
  `;

  @state() private gpus: Gpu[] = [];
  @state() private system: SystemInfo | null = null;
  @state() private sample: Sample | null = null;
  /** Per-metric history rings, keyed by metric key. */
  @state() private history = new Map<string, number[]>();

  #unlisten?: () => void;
  #onChange = () => this.syncSampler();
  /** Sampling interval from settings (ms); loaded on connect. */
  #intervalMs = 1000;

  override connectedCallback(): void {
    super.connectedCallback();
    void this.loadGpus();
    void this.loadSystem();
    void this.loadInterval();
    void this.listen();
    document.addEventListener("visibilitychange", this.#onChange);
    window.addEventListener("focus", this.#onChange);
    window.addEventListener("blur", this.#onChange);
    this.syncSampler();
  }

  override disconnectedCallback(): void {
    super.disconnectedCallback();
    document.removeEventListener("visibilitychange", this.#onChange);
    window.removeEventListener("focus", this.#onChange);
    window.removeEventListener("blur", this.#onChange);
    this.#unlisten?.();
    this.#unlisten = undefined;
    void monitorStop();
  }

  /** Run the sampler only while mounted, visible, and focused. */
  private syncSampler(): void {
    const active = this.isConnected && !document.hidden && document.hasFocus();
    void (active ? monitorStart(this.#intervalMs) : monitorStop());
  }

  private async loadInterval(): Promise<void> {
    try {
      this.#intervalMs = (await getSettings()).monitorIntervalMs;
      this.syncSampler(); // restart at the configured rate if already running
    } catch {
      // keep the default
    }
  }

  private async loadGpus(): Promise<void> {
    try {
      this.gpus = await getGpus();
    } catch {
      this.gpus = [];
    }
  }

  private async loadSystem(): Promise<void> {
    try {
      this.system = await getSystemInfo();
    } catch {
      this.system = null;
    }
  }

  private async listen(): Promise<void> {
    const unlisten = await subscribe("monitor://sample", (event) => this.onSample(event.payload));
    if (this.isConnected) this.#unlisten = unlisten;
    else unlisten();
  }

  private onSample(sample: Sample): void {
    this.sample = sample;
    const push = (key: string, value: number) => {
      const next = [...(this.history.get(key) ?? []), value].slice(-HISTORY);
      this.history.set(key, next);
    };
    push("cpu", sample.cpuPercent);
    push("ram", memPercent(sample));
    push("net-rx", sample.netRxBps);
    push("net-tx", sample.netTxBps);
    push("disk-r", sample.diskReadBps);
    push("disk-w", sample.diskWriteBps);
    if (sample.gpuPercent != null) push("gpu", sample.gpuPercent);
    if (sample.vramUsedBytes != null) push("vram", sample.vramUsedBytes);
    this.history = new Map(this.history); // trigger reactivity
  }

  render() {
    return html`
      <view-page heading="Monitor" tagline="A minimal, real-time read on CPU, RAM, network, disk, and GPU.">
        <div class="body">
          ${this.renderSystem()} ${this.renderGpus()}
          <section aria-labelledby="live-h">
            <h2 id="live-h">Live</h2>
            ${this.sample ? this.renderMetrics(this.sample) : this.renderWaiting()}
          </section>
        </div>
      </view-page>
    `;
  }

  private renderWaiting() {
    return html`<p class="muted" role="status">
      ${document.hasFocus() ? "Starting sampler…" : "Paused — focus the window to resume."}
    </p>`;
  }

  private async openTaskMgr(): Promise<void> {
    try {
      await openTaskManager();
    } catch {
      // best-effort
    }
  }

  private renderSystem() {
    const s = this.system;
    const os = s ? [s.osName, s.osVersion].filter(Boolean).join(" ") : "";
    // icon, label, value, wide?
    const cards: Array<[string, string, string, boolean]> = s
      ? [
          ["🪟", "OS", os || "—", false],
          ["🧠", "CPU", s.cpu || "—", true],
          ["🧵", "Threads", String(s.cpuThreads), false],
          ["💾", "Memory", formatBytes(s.memTotalBytes) ?? "—", false],
          ["🖥️", "Host", s.hostname || "—", false],
          ["⚙️", "Kernel", s.kernelVersion || "—", false],
        ]
      : [];
    return html`
      <section aria-labelledby="sys-h">
        <div class="sys-head">
          <h2 id="sys-h">System</h2>
          <button class="tm-btn" @click=${this.openTaskMgr} title="Open Windows Task Manager">
            Open Task Manager
          </button>
        </div>
        ${s
          ? html`<div class="sysgrid">
              ${cards.map(
                ([icon, label, value, wide]) => html`<div class="syscard ${wide ? "wide" : ""}">
                  <span class="ico" aria-hidden="true">${icon}</span>
                  <div class="sys-text">
                    <div class="sys-label">${label}</div>
                    <div class="sys-val" title=${value}>${value}</div>
                  </div>
                </div>`,
              )}
            </div>`
          : html`<p class="muted">System info unavailable.</p>`}
      </section>
    `;
  }

  private renderGpus() {
    return html`
      <section aria-labelledby="gpu-h">
        <h2 id="gpu-h">Graphics</h2>
        ${this.gpus.length === 0
          ? html`<p class="muted">No GPU detected.</p>`
          : html`<div class="gpus">
              ${this.gpus.map(
                (g) => html`
                  <div class="gpu">
                    <div class="gpu-name">${g.name}</div>
                    <div class="muted">
                      Driver ${g.driverVersion}${g.driverDate ? ` · ${g.driverDate}` : ""}
                    </div>
                  </div>
                `,
              )}
            </div>`}
      </section>
    `;
  }

  private renderMetrics(s: Sample) {
    const metrics: Metric[] = [
      { key: "cpu", label: "CPU", value: `${s.cpuPercent.toFixed(0)}%`, history: this.hist("cpu"), max: 100 },
      {
        key: "ram",
        label: "Memory",
        value: `${formatBytes(s.memUsedBytes)} / ${formatBytes(s.memTotalBytes)}`,
        history: this.hist("ram"),
        // Used RAM sits in a narrow band well above zero; min-max so the graph
        // shows real movement instead of a flat line near mid-height.
        minMax: true,
      },
      { key: "net-rx", label: "Network ↓", value: `${formatBytes(s.netRxBps)}/s`, history: this.hist("net-rx") },
      { key: "net-tx", label: "Network ↑", value: `${formatBytes(s.netTxBps)}/s`, history: this.hist("net-tx") },
      { key: "disk-r", label: "Disk read", value: `${formatBytes(s.diskReadBps)}/s`, history: this.hist("disk-r") },
      { key: "disk-w", label: "Disk write", value: `${formatBytes(s.diskWriteBps)}/s`, history: this.hist("disk-w") },
      {
        key: "gpu",
        label: "GPU",
        value: s.gpuPercent != null ? `${s.gpuPercent.toFixed(0)}%` : "unavailable",
        history: this.hist("gpu"),
        max: 100,
      },
      {
        key: "vram",
        label: "VRAM",
        value:
          s.vramUsedBytes != null
            ? `${formatBytes(s.vramUsedBytes)}${s.vramTotalBytes ? ` / ${formatBytes(s.vramTotalBytes)}` : ""}`
            : "unavailable",
        history: this.hist("vram"),
      },
    ];
    return html`<div class="metrics">${metrics.map((m) => this.renderMetric(m))}</div>`;
  }

  private renderMetric(m: Metric) {
    return html`
      <div class="metric">
        <div class="metric-label">${m.label}</div>
        <div class="metric-value">${m.value}</div>
        ${sparkline(m.history, { max: m.max, minMax: m.minMax })}
      </div>
    `;
  }

  private hist(key: string): number[] {
    return this.history.get(key) ?? [];
  }
}

function memPercent(s: Sample): number {
  return s.memTotalBytes > 0 ? (s.memUsedBytes / s.memTotalBytes) * 100 : 0;
}

interface SparkOpts {
  /** Fixed axis max; the axis runs 0..max (else 0..max-of-window). */
  max?: number;
  /** Scale to the window's min..max instead of a zero baseline. */
  minMax?: boolean;
}

/**
 * Tiny smoothed sparkline of the given values. By default the axis runs from
 * zero to `max` (or the window's peak); `minMax` instead fits the window's own
 * min..max range so a narrow-band signal (memory) shows its real shape. The path
 * is a Catmull-Rom curve so the 1 Hz samples read as a smooth line, not a
 * zig-zag.
 */
function sparkline(values: number[], opts: SparkOpts = {}) {
  if (values.length < 2) return nothing;
  const w = 100;
  const h = 32;
  const pad = 3; // keep peaks/troughs off the stroke-clipped edges

  let lo: number;
  let hi: number;
  if (opts.minMax) {
    lo = Math.min(...values);
    hi = Math.max(...values);
    // Don't amplify noise: require the band to span at least ~2% of magnitude,
    // centering a near-flat signal rather than blowing it up to full height.
    const minRange = Math.max(Math.abs(hi), 1) * 0.02;
    if (hi - lo < minRange) {
      const mid = (hi + lo) / 2;
      lo = mid - minRange / 2;
      hi = mid + minRange / 2;
    }
  } else {
    lo = 0;
    hi = Math.max(opts.max ?? 0, ...values, 1);
  }
  const range = hi - lo || 1;

  const step = w / (HISTORY - 1);
  const pts = values.map((v, i) => ({
    x: i * step,
    y: pad + (h - 2 * pad) * (1 - (clamp(v, lo, hi) - lo) / range),
  }));

  return html`<svg class="spark" viewBox="0 0 ${w} ${h}" preserveAspectRatio="none" aria-hidden="true">
    <path d=${smoothPath(pts)}></path>
  </svg>`;
}

function clamp(v: number, lo: number, hi: number): number {
  return v < lo ? lo : v > hi ? hi : v;
}

/** Catmull-Rom spline through the points as an SVG cubic-bezier path. */
function smoothPath(pts: ReadonlyArray<{ x: number; y: number }>): string {
  if (pts.length < 2) return "";
  const f = (n: number) => n.toFixed(2);
  let d = `M ${f(pts[0].x)},${f(pts[0].y)}`;
  for (let i = 0; i < pts.length - 1; i++) {
    const p0 = pts[i === 0 ? 0 : i - 1];
    const p1 = pts[i];
    const p2 = pts[i + 1];
    const p3 = pts[i + 2 < pts.length ? i + 2 : pts.length - 1];
    const c1x = p1.x + (p2.x - p0.x) / 6;
    const c1y = p1.y + (p2.y - p0.y) / 6;
    const c2x = p2.x - (p3.x - p1.x) / 6;
    const c2y = p2.y - (p3.y - p1.y) / 6;
    d += ` C ${f(c1x)},${f(c1y)} ${f(c2x)},${f(c2y)} ${f(p2.x)},${f(p2.y)}`;
  }
  return d;
}

declare global {
  interface HTMLElementTagNameMap {
    "monitor-view": MonitorView;
  }
}
