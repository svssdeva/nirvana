import { LitElement, html, css, nothing } from "lit";
import { customElement, state } from "lit/decorators.js";
import type { AppError, Drive, Game } from "../../ipc";
import {
  computeGameSizes,
  getLibrary,
  listDrives,
  openInstallFolder,
  subscribe,
  toAppError,
  uninstallGame,
} from "../../ipc";
import { formatBytes } from "../../format";
import "../view-page";
import "../empty-state";

type Status = "loading" | "ready" | "error";

const TOP_N = 20;

/**
 * Disk view (design.md, Task 15): per-drive capacity and the biggest games.
 * Loads drives + the library, runs an accurate-size pass on demand (manifest
 * sizes stand in until then), and updates rows live from `size://progress`.
 * Read-only — actions are "open install folder" and the store's own uninstall
 * flow; Nirvana never deletes files.
 */
@customElement("disk-view")
export class DiskView extends LitElement {
  static styles = css`
    :host {
      display: block;
    }
    .toolbar {
      display: flex;
      align-items: center;
      gap: 16px;
      padding: 0 48px 24px;
    }
    .scan {
      font: 700 18px/1.25 var(--font-body);
      letter-spacing: 0.45px;
      color: var(--on-primary);
      background: var(--primary);
      border: none;
      border-radius: var(--rounded-full);
      padding: 12px 28px;
      min-height: 48px;
      cursor: pointer;
    }
    .scan:active:not(:disabled) {
      background: var(--primary-pressed);
    }
    .scan:focus-visible {
      outline: 2px solid var(--on-primary);
      outline-offset: 2px;
    }
    .scan:disabled {
      background: var(--surface-soft);
      color: var(--ash-light);
      cursor: default;
    }
    .hint {
      font-size: 14px;
      color: var(--on-surface-muted);
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
    .drives {
      display: grid;
      grid-template-columns: repeat(auto-fill, minmax(240px, 1fr));
      gap: 16px;
    }
    .drive {
      background: var(--surface);
      border: 1px solid var(--hairline);
      border-radius: var(--rounded-md);
      padding: 16px;
    }
    .row-between {
      display: flex;
      justify-content: space-between;
      align-items: baseline;
      gap: 12px;
    }
    .drive-name {
      font-weight: 600;
      font-size: 16px;
      color: var(--on-surface);
    }
    .muted {
      font-size: 14px;
      color: var(--on-surface-muted);
    }
    .bar {
      height: 6px;
      background: var(--surface-elevated);
      border-radius: var(--rounded-full);
      overflow: hidden;
      margin: 10px 0 6px;
    }
    .bar-fill {
      height: 100%;
      background: var(--primary);
      border-radius: var(--rounded-full);
    }
    ol.games {
      list-style: none;
      margin: 0;
      padding: 0;
      display: grid;
      gap: 12px;
    }
    .game {
      background: var(--surface);
      border: 1px solid var(--hairline);
      border-radius: var(--rounded-md);
      padding: 12px 16px;
    }
    .g-name {
      font-weight: 600;
      font-size: 16px;
      color: var(--on-surface);
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
    }
    .badge {
      flex: none;
      background: var(--primary);
      color: var(--on-primary);
      font-size: 12px;
      font-weight: 500;
      padding: 2px 10px;
      border-radius: var(--rounded-full);
    }
    .g-meta {
      display: flex;
      align-items: center;
      gap: 16px;
      margin-top: 6px;
      font-size: 14px;
      color: var(--on-surface-muted);
    }
    .g-size {
      font-weight: 600;
      color: var(--on-surface);
    }
    .g-actions {
      margin-left: auto;
      display: flex;
      gap: 12px;
    }
    .link {
      background: none;
      border: none;
      padding: 0;
      font: 500 14px/1.5 var(--font-body);
      color: var(--primary);
      cursor: pointer;
    }
    .link:focus-visible {
      outline: 2px solid var(--primary);
      outline-offset: 2px;
    }
    .error {
      max-width: 520px;
      border: 1px solid var(--hairline);
      border-left: 3px solid var(--danger);
      border-radius: var(--rounded-md);
      padding: 24px;
      color: var(--on-surface-muted);
    }
    @media (max-width: 768px) {
      .toolbar,
      .body {
        padding-left: 24px;
        padding-right: 24px;
      }
    }
  `;

  @state() private drives: Drive[] = [];
  @state() private games: Game[] = [];
  @state() private status: Status = "loading";
  @state() private error: AppError | null = null;
  @state() private computing = false;

  #unlisten?: () => void;

  override connectedCallback(): void {
    super.connectedCallback();
    void this.listenForSizes();
    void this.load();
  }

  override disconnectedCallback(): void {
    super.disconnectedCallback();
    this.#unlisten?.();
    this.#unlisten = undefined;
  }

  private async listenForSizes(): Promise<void> {
    const unlisten = await subscribe("size://progress", (event) => {
      const { id, sizeBytes } = event.payload;
      this.games = this.games.map((g) => (g.id === id ? { ...g, sizeBytes } : g));
    });
    if (this.isConnected) this.#unlisten = unlisten;
    else unlisten();
  }

  private async load(): Promise<void> {
    this.status = "loading";
    try {
      const [drives, games] = await Promise.all([listDrives(), getLibrary()]);
      this.drives = drives;
      this.games = games;
      this.status = "ready";
    } catch (e) {
      this.error = toAppError(e);
      this.status = "error";
    }
  }

  private async compute(): Promise<void> {
    if (this.computing) return;
    this.computing = true;
    try {
      await computeGameSizes(); // rows update live via size://progress
    } catch (e) {
      this.error = toAppError(e);
    } finally {
      this.computing = false;
    }
  }

  private async open(id: number): Promise<void> {
    try {
      await openInstallFolder(id);
    } catch (e) {
      this.error = toAppError(e);
    }
  }

  private async uninstall(id: number): Promise<void> {
    try {
      await uninstallGame(id);
    } catch (e) {
      this.error = toAppError(e);
    }
  }

  /** Games sorted by size desc (unknown sizes last), capped to the top N. */
  private biggestGames(): Game[] {
    return [...this.games]
      .sort((a, b) => (b.sizeBytes ?? -1) - (a.sizeBytes ?? -1))
      .slice(0, TOP_N);
  }

  render() {
    return html`
      <view-page heading="Disk" tagline="See where space goes — per drive and per game.">
        <div class="toolbar">
          <button class="scan" @click=${this.compute} ?disabled=${this.computing}>
            ${this.computing ? "Measuring…" : "Measure sizes"}
          </button>
          <span class="hint">
            Sizes start from store manifests; "Measure sizes" walks each install for the exact total.
          </span>
        </div>
        <div class="body">${this.renderBody()}</div>
      </view-page>
    `;
  }

  private renderBody() {
    if (this.status === "error") {
      return html`<div class="error" role="alert">
        ${this.error?.message ?? "Couldn't load disk info."}
      </div>`;
    }
    if (this.status === "loading") {
      return html`<p class="muted" role="status" aria-busy="true">Loading drives and library…</p>`;
    }
    return html`${this.renderDrives()} ${this.renderGames()}`;
  }

  private renderDrives() {
    if (this.drives.length === 0) return nothing;
    return html`
      <section aria-labelledby="drives-h">
        <h2 id="drives-h">Drives</h2>
        <div class="drives">
          ${this.drives.map((d) => {
            const used = Math.max(0, d.totalBytes - d.freeBytes);
            const pct = d.totalBytes > 0 ? (used / d.totalBytes) * 100 : 0;
            return html`
              <div class="drive">
                <div class="row-between">
                  <span class="drive-name">${d.label || d.mount}</span>
                  <span class="muted">${formatBytes(d.freeBytes)} free</span>
                </div>
                <div
                  class="bar"
                  role="progressbar"
                  aria-valuenow=${Math.round(pct)}
                  aria-valuemin="0"
                  aria-valuemax="100"
                >
                  <div class="bar-fill" style="width:${pct}%"></div>
                </div>
                <span class="muted">${formatBytes(used)} of ${formatBytes(d.totalBytes)} used</span>
              </div>
            `;
          })}
        </div>
      </section>
    `;
  }

  private renderGames() {
    const games = this.biggestGames();
    if (games.length === 0) {
      return html`<empty-state
        glyph="◴"
        label="No games yet"
        hint="Scan your library first, then come back to see what's taking up space."
      ></empty-state>`;
    }
    const max = games[0]?.sizeBytes ?? 0;
    return html`
      <section aria-labelledby="games-h">
        <h2 id="games-h">Biggest games</h2>
        <ol class="games">
          ${games.map((g) => {
            const pct = max > 0 && g.sizeBytes ? (g.sizeBytes / max) * 100 : 0;
            const label = g.source[0].toUpperCase() + g.source.slice(1);
            return html`
              <li class="game">
                <div class="row-between">
                  <span class="g-name" title=${g.name}>${g.name}</span>
                  <span class="badge">${label}</span>
                </div>
                <div class="bar"><div class="bar-fill" style="width:${pct}%"></div></div>
                <div class="g-meta">
                  <span class="g-size">${formatBytes(g.sizeBytes) ?? "—"}</span>
                  ${g.drive ? html`<span>${g.drive}</span>` : nothing}
                  <span class="g-actions">
                    <button class="link" @click=${() => this.open(g.id)}>Open folder</button>
                    ${g.source === "steam"
                      ? html`<button class="link" @click=${() => this.uninstall(g.id)}>Uninstall</button>`
                      : nothing}
                  </span>
                </div>
              </li>
            `;
          })}
        </ol>
      </section>
    `;
  }
}

declare global {
  interface HTMLElementTagNameMap {
    "disk-view": DiskView;
  }
}
