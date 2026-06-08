import { LitElement, html, css, nothing } from "lit";
import { customElement, state } from "lit/decorators.js";
import { consume } from "@lit/context";
import { unsafeHTML } from "lit/directives/unsafe-html.js";
import { storeContext, type Store, type Theme } from "../store";
import {
  appVersion,
  createCollection,
  deleteCollection,
  getDonationInfo,
  getSettings,
  listCollections,
  ping,
  renameCollection,
  resetDatabase,
  seedDummyGames,
  setSetting,
  toAppError,
  type Collection,
  type DonationInfo,
  type Settings,
} from "../ipc";
import "./view-page";

type PingState =
  | { status: "idle" }
  | { status: "ok"; value: string }
  | { status: "error"; message: string };

/**
 * Settings: Preferences (theme, monitor interval, watch folders, SteamGridDB),
 * the "Support Nirvana" donation section, and an IPC smoke test. Theme persists
 * via localStorage (instant, pre-DB); the rest persist in the DB `setting` table.
 */
@customElement("settings-view")
export class SettingsView extends LitElement {
  static styles = css`
    /* Balanced, centered two-column layout — fills the canvas without sprawling
       into thin columns or hugging the left edge. Collapses to one column when
       narrow. align-items:start keeps short panels from stretching. */
    .panels {
      max-width: 1000px;
      margin: 0 auto;
      display: grid;
      gap: 20px;
      grid-template-columns: repeat(2, minmax(0, 1fr));
      align-items: start;
      padding: 8px 48px 24px;
    }
    .panel {
      padding: 24px 28px;
      background: var(--surface);
      border: 1px solid var(--hairline);
      border-radius: 12px;
      box-shadow: 0 1px 2px rgba(0, 0, 0, 0.05);
    }
    @media (max-width: 900px) {
      .panels {
        grid-template-columns: 1fr;
      }
    }
    h2 {
      margin: 0 0 4px;
      font-family: var(--font-display);
      font-weight: 300;
      font-size: 22px;
      line-height: 1.25;
      color: var(--on-surface);
    }
    p {
      margin: 0 0 16px;
      font-size: 16px;
      line-height: 1.5;
      color: var(--on-surface-muted);
    }
    .field {
      display: grid;
      gap: 8px;
      margin: 0 0 20px;
    }
    .field-label {
      font-size: 14px;
      font-weight: 600;
      color: var(--on-surface);
    }
    .row {
      display: flex;
      flex-wrap: wrap;
      align-items: center;
      gap: 8px;
    }
    .fpill {
      font: 700 14px/1.25 var(--font-body);
      letter-spacing: 0.324px;
      padding: 8px 16px;
      border-radius: var(--rounded-full);
      border: 1px solid var(--hairline);
      background: var(--surface-elevated);
      color: var(--on-surface-muted);
      cursor: pointer;
    }
    .fpill.active {
      background: var(--primary);
      border-color: var(--primary);
      color: var(--on-primary);
    }
    .fpill:focus-visible,
    .pill:focus-visible {
      outline: 2px solid var(--primary);
      outline-offset: 2px;
    }
    select,
    input[type="text"] {
      padding: 8px 12px;
      font: 400 16px/1.25 var(--font-body);
      color: var(--on-surface);
      background: var(--bg);
      border: 1px solid var(--hairline);
      border-radius: var(--rounded-sm);
    }
    input[type="text"] {
      flex: 1 1 220px;
      min-width: 0;
    }
    .folder {
      display: flex;
      align-items: center;
      justify-content: space-between;
      gap: 12px;
      padding: 8px 12px;
      background: var(--surface-elevated);
      border-radius: var(--rounded-sm);
      font-size: 14px;
      color: var(--on-surface);
      word-break: break-all;
    }
    .x {
      flex: none;
      border: none;
      background: none;
      color: var(--danger);
      font-size: 16px;
      cursor: pointer;
    }
    .toggle {
      display: flex;
      align-items: center;
      gap: 10px;
      font-size: 16px;
      color: var(--on-surface);
    }
    /* iOS-style toggle switch. */
    .switch {
      display: inline-flex;
      align-items: center;
      gap: 12px;
      cursor: pointer;
      font-size: 16px;
      color: var(--on-surface);
    }
    .switch input {
      position: absolute;
      opacity: 0;
      width: 0;
      height: 0;
    }
    .switch .track {
      position: relative;
      flex: none;
      width: 42px;
      height: 24px;
      border-radius: var(--rounded-full);
      background: var(--hairline);
      transition: background 0.15s ease;
    }
    .switch .track::after {
      content: "";
      position: absolute;
      top: 2px;
      left: 2px;
      width: 20px;
      height: 20px;
      border-radius: 50%;
      background: #fff;
      box-shadow: 0 1px 2px rgba(0, 0, 0, 0.35);
      transition: transform 0.15s ease;
    }
    .switch input:checked + .track {
      background: var(--primary);
    }
    .switch input:checked + .track::after {
      transform: translateX(18px);
    }
    .switch input:focus-visible + .track {
      outline: 2px solid var(--primary);
      outline-offset: 2px;
    }
    .warn {
      margin: 8px 0 0;
      font-size: 14px;
      color: var(--on-surface-muted);
      border-left: 3px solid var(--warning, #c81b3a);
      padding-left: 10px;
    }
    .pill {
      border: none;
      border-radius: var(--rounded-full, 9999px);
      padding: 10px 22px;
      font: 700 14px/1.25 var(--font-body);
      letter-spacing: 0.324px;
      background: var(--primary, #0070d1);
      color: var(--on-primary, #fff);
      cursor: pointer;
    }
    .pill:active {
      background: var(--primary-pressed, #0064b7);
    }
    .panel.danger {
      border-color: var(--danger);
    }
    .danger-btn {
      background: var(--danger);
      color: #fff;
    }
    .danger-btn:active {
      filter: brightness(0.92);
    }
    /* QR sits on white in both themes so any UPI app can scan it. */
    .qr {
      width: 200px;
      background: #ffffff;
      padding: 12px;
      border-radius: var(--rounded-md, 8px);
      margin: 0 0 12px;
    }
    .qr svg {
      display: block;
      width: 100%;
      height: auto;
    }
    .qr-cap {
      margin: 0 0 20px;
      font-size: 14px;
      color: var(--on-surface-muted);
    }
    .upi-row {
      display: flex;
      flex-wrap: wrap;
      align-items: center;
      gap: 12px;
    }
    .upi-id {
      flex: 1 1 auto;
      min-width: 0;
      padding: 10px 14px;
      background: var(--surface-elevated);
      border: 1px solid var(--hairline);
      border-radius: var(--rounded-sm, 4px);
      font-size: 16px;
      color: var(--on-surface);
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
      user-select: all;
    }
    .result {
      margin: 16px 0 0;
      font-size: 14px;
    }
    .result.ok {
      color: var(--on-surface-muted);
    }
    .result.error {
      color: var(--danger);
    }
    .version {
      margin: 8px 48px 24px;
      font-size: 12px;
      color: var(--on-surface-muted);
    }
    .muted {
      color: var(--on-surface-muted);
    }
    .kv {
      display: grid;
      gap: 6px;
    }
    .kv-row {
      display: flex;
      justify-content: space-between;
      gap: 16px;
      font-size: 14px;
      color: var(--on-surface);
    }
    .kv-row span:last-child {
      text-align: right;
    }
    .coll-row {
      display: flex;
      flex-wrap: wrap;
      align-items: center;
      gap: 8px;
      padding: 8px 12px;
      background: var(--surface-elevated);
      border-radius: var(--rounded-sm);
      margin: 0 0 8px;
    }
    .coll-name {
      flex: 1 1 auto;
      min-width: 0;
      font-size: 15px;
      color: var(--on-surface);
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
    }
    .coll-rename-input {
      flex: 1 1 160px;
      min-width: 0;
      padding: 6px 10px;
      font: 400 15px/1.25 var(--font-body);
      color: var(--on-surface);
      background: var(--bg);
      border: 1px solid var(--hairline);
      border-radius: var(--rounded-sm);
    }
    .coll-create-row {
      margin-top: 12px;
    }
    @media (max-width: 768px) {
      .panels {
        padding: 8px 24px 24px;
      }
      .version {
        margin-left: 24px;
      }
    }
  `;

  @consume({ context: storeContext, subscribe: true })
  private store?: Store;

  @state() private settings: Settings | null = null;
  @state() private donation: DonationInfo | null = null;
  @state() private copied = false;
  @state() private ping: PingState = { status: "idle" };
  /** Bound value of the "add watch folder" input. */
  @state() private newFolder = "";
  @state() private version = "";
  @state() private seedMsg = "";
  @state() private confirmReset = false;
  @state() private resetMsg = "";
  #resetTimer?: ReturnType<typeof setTimeout>;

  @state() private collections: Collection[] = [];
  @state() private newCollName = "";
  @state() private collErr = "";
  @state() private renamingId: number | null = null;
  @state() private renamingValue = "";
  @state() private confirmDeleteId: number | null = null;
  #confirmDeleteTimer?: ReturnType<typeof setTimeout>;

  /** Vite sets this only in `tauri dev` builds. */
  private readonly isDev = import.meta.env.DEV;

  #copiedTimer?: ReturnType<typeof setTimeout>;
  #storeUnsub?: () => void;

  override connectedCallback(): void {
    super.connectedCallback();
    void this.loadSettings();
    void this.loadDonation();
    void this.loadCollections();
    void appVersion()
      .then((v) => (this.version = v))
      .catch(() => {});
    // Re-render when the theme changes elsewhere (the store object is stable, so
    // @consume won't fire on internal emits — subscribe directly, like app-nav).
    this.#storeUnsub = this.store?.subscribe(() => this.requestUpdate());
  }

  override disconnectedCallback(): void {
    super.disconnectedCallback();
    clearTimeout(this.#copiedTimer);
    clearTimeout(this.#resetTimer);
    clearTimeout(this.#confirmDeleteTimer);
    this.#storeUnsub?.();
  }

  private async loadSettings(): Promise<void> {
    try {
      this.settings = await getSettings();
    } catch {
      this.settings = null;
    }
  }

  private async loadDonation(): Promise<void> {
    try {
      this.donation = await getDonationInfo();
    } catch {
      this.donation = null;
    }
  }

  private async loadCollections(): Promise<void> {
    try {
      this.collections = await listCollections();
    } catch {
      this.collections = [];
    }
  }

  private async addCollection(): Promise<void> {
    const name = this.newCollName.trim();
    if (!name) return;
    try {
      await createCollection(name);
      this.newCollName = "";
      this.collErr = "";
      await this.loadCollections();
    } catch (e) {
      this.collErr = toAppError(e).message;
    }
  }

  private async renameCollection_(id: number, name: string): Promise<void> {
    const n = name.trim();
    if (!n) return;
    try {
      await renameCollection(id, n);
      this.renamingId = null;
      this.renamingValue = "";
      this.collErr = "";
      await this.loadCollections();
    } catch (e) {
      this.collErr = toAppError(e).message;
    }
  }

  private async deleteCollection_(id: number): Promise<void> {
    try {
      await deleteCollection(id);
      this.confirmDeleteId = null;
      clearTimeout(this.#confirmDeleteTimer);
      await this.loadCollections();
    } catch (e) {
      this.collErr = toAppError(e).message;
    }
  }

  private setTheme(theme: Theme): void {
    this.store?.setTheme(theme); // app-root applies it to <html> + localStorage
  }

  private async setInterval(event: Event): Promise<void> {
    const ms = Number((event.target as HTMLSelectElement).value);
    if (this.settings) this.settings = { ...this.settings, monitorIntervalMs: ms };
    await this.persist("monitorIntervalMs", String(ms));
  }

  private async toggleSteamGridDb(event: Event): Promise<void> {
    const enabled = (event.target as HTMLInputElement).checked;
    if (this.settings) this.settings = { ...this.settings, steamgriddbEnabled: enabled };
    await this.persist("steamgriddbEnabled", enabled ? "true" : "false");
  }

  private async addFolder(): Promise<void> {
    const path = this.newFolder.trim();
    if (!path || !this.settings) return;
    const folders = [...new Set([...this.settings.watchFolders, path])];
    this.settings = { ...this.settings, watchFolders: folders };
    this.newFolder = "";
    await this.persist("watchFolders", JSON.stringify(folders));
  }

  private async removeFolder(path: string): Promise<void> {
    if (!this.settings) return;
    const folders = this.settings.watchFolders.filter((f) => f !== path);
    this.settings = { ...this.settings, watchFolders: folders };
    await this.persist("watchFolders", JSON.stringify(folders));
  }

  private async persist(key: string, value: string): Promise<void> {
    try {
      await setSetting(key, value);
    } catch {
      // best-effort; reload to resync if a write fails
      void this.loadSettings();
    }
  }

  private renderDevTools() {
    return html`
      <div class="panel">
        <h2>Developer (dev build only)</h2>
        <p>Populate the library with sample games to exercise the UI locally.</p>
        <button class="pill" @click=${this.seed}>Seed 50+ dummy games</button>
        ${this.seedMsg
          ? html`<p class="result ok" role="status">${this.seedMsg}</p>`
          : nothing}
      </div>
    `;
  }

  private renderCollections() {
    return html`
      <section class="panel" aria-labelledby="coll-h">
        <h2 id="coll-h">Collections</h2>
        <p>Group your games into named collections you can filter by.</p>

        ${this.collections.length === 0
          ? html`<p class="result ok" role="status">No collections yet.</p>`
          : this.collections.map((c) => {
              const isRenaming = this.renamingId === c.id;
              const isConfirming = this.confirmDeleteId === c.id;
              return html`
                <div class="coll-row">
                  ${isRenaming
                    ? html`
                        <input
                          type="text"
                          class="coll-rename-input"
                          aria-label="New name for ${c.name}"
                          .value=${this.renamingValue}
                          @input=${(e: Event) =>
                            (this.renamingValue = (e.target as HTMLInputElement).value)}
                          @keydown=${(e: KeyboardEvent) => {
                            if (e.key === "Enter") void this.renameCollection_(c.id, this.renamingValue);
                            if (e.key === "Escape") {
                              this.renamingId = null;
                              this.renamingValue = "";
                            }
                          }}
                        />
                        <button
                          class="fpill active"
                          @click=${() => void this.renameCollection_(c.id, this.renamingValue)}
                        >
                          Save
                        </button>
                        <button
                          class="fpill"
                          @click=${() => {
                            this.renamingId = null;
                            this.renamingValue = "";
                          }}
                        >
                          Cancel
                        </button>
                      `
                    : html`
                        <span class="coll-name">${c.name}</span>
                        <button
                          class="fpill"
                          aria-label="Rename ${c.name}"
                          @click=${() => {
                            this.renamingId = c.id;
                            this.renamingValue = c.name;
                          }}
                        >
                          Rename
                        </button>
                        <button
                          class="fpill ${isConfirming ? "danger-btn" : ""}"
                          aria-label="${isConfirming ? "Click again to confirm delete" : "Delete " + c.name}"
                          @click=${() => {
                            if (!isConfirming) {
                              this.confirmDeleteId = c.id;
                              clearTimeout(this.#confirmDeleteTimer);
                              this.#confirmDeleteTimer = setTimeout(
                                () => (this.confirmDeleteId = null),
                                4000,
                              );
                            } else {
                              clearTimeout(this.#confirmDeleteTimer);
                              void this.deleteCollection_(c.id);
                            }
                          }}
                        >
                          ${isConfirming ? "Confirm delete" : "Delete"}
                        </button>
                      `}
                </div>
              `;
            })}

        <div class="row coll-create-row">
          <input
            type="text"
            placeholder="New collection name"
            aria-label="New collection name"
            .value=${this.newCollName}
            @input=${(e: Event) => (this.newCollName = (e.target as HTMLInputElement).value)}
            @keydown=${(e: KeyboardEvent) => e.key === "Enter" && void this.addCollection()}
          />
          <button class="fpill" @click=${() => void this.addCollection()}>Create</button>
        </div>

        ${this.collErr
          ? html`<p class="result error" role="alert">${this.collErr}</p>`
          : nothing}
      </section>
    `;
  }

  private renderDanger() {
    return html`
      <section class="panel danger" aria-labelledby="danger-h">
        <h2 id="danger-h">Delete database</h2>
        <p>
          Clears all discovered games, tags, favorites, custom covers, and saved
          settings (watch folders, preferences). Your actual game files are never
          touched. This can't be undone.
        </p>
        <button class="pill danger-btn" @click=${this.resetDb}>
          ${this.confirmReset ? "Click again to confirm" : "Delete database"}
        </button>
        ${this.resetMsg ? html`<p class="result ok" role="status">${this.resetMsg}</p>` : nothing}
      </section>
    `;
  }

  private async resetDb(): Promise<void> {
    // Two-step confirm: first click arms, second (within 4s) commits.
    if (!this.confirmReset) {
      this.confirmReset = true;
      clearTimeout(this.#resetTimer);
      this.#resetTimer = setTimeout(() => (this.confirmReset = false), 4000);
      return;
    }
    clearTimeout(this.#resetTimer);
    this.confirmReset = false;
    try {
      await resetDatabase();
      this.settings = await getSettings(); // reflect wiped settings
      this.resetMsg = "Database cleared. Open the Library tab and rescan.";
    } catch (e) {
      this.resetMsg = `Failed: ${toAppError(e).message}`;
    }
  }

  private async seed(): Promise<void> {
    try {
      const n = await seedDummyGames();
      this.seedMsg = `Seeded ${n} games — open the Library tab to see them.`;
    } catch (e) {
      this.seedMsg = `Failed: ${toAppError(e).message}`;
    }
  }

  private async runPing(): Promise<void> {
    try {
      this.ping = { status: "ok", value: await ping(false) };
    } catch (e) {
      this.ping = { status: "error", message: toAppError(e).message };
    }
  }

  private renderAbout() {
    const stack: Array<[string, string]> = [
      ["Shell", "Tauri 2 (Rust core + system WebView2)"],
      ["Frontend", "Lit 3 + Vite + TypeScript"],
      ["Storage", "SQLite (rusqlite)"],
      ["System / GPU", "windows crate (WMI · PDH · DXGI), sysinfo"],
      ["Art / extras", "image, qrcode, keyring (opt-in SteamGridDB)"],
    ];
    return html`
      <section class="panel" aria-labelledby="about-h">
        <h2 id="about-h">About</h2>
        <p>
          Nirvana is a fully-offline Windows launcher that unifies your Steam,
          Epic, and local games — with disk insight, a GPU/driver panel, and a
          minimal real-time system monitor. No accounts, no telemetry.
        </p>
        <div class="kv">
          ${stack.map(
            ([k, v]) => html`<div class="kv-row"><span class="muted">${k}</span><span>${v}</span></div>`,
          )}
        </div>
      </section>
    `;
  }

  render() {
    return html`
      <view-page heading="Settings" tagline="Preferences, support, and diagnostics.">
        <div class="panels">
          ${this.renderPreferences()} ${this.renderAbout()} ${this.renderSupport()}
          ${this.renderCollections()} ${this.renderDanger()}
          ${this.isDev
            ? html`<div class="panel">
                  <h2>IPC diagnostics</h2>
                  <p>Round-trips the Rust ping command through the typed IPC seam.</p>
                  <button class="pill" @click=${this.runPing}>Test IPC</button>
                  ${this.renderPingResult()}
                </div>
                ${this.renderDevTools()}`
            : nothing}
        </div>
        <p class="version" role="contentinfo">
          Nirvana${this.version ? ` v${this.version}` : ""} · alpha
        </p>
      </view-page>
    `;
  }

  private renderPreferences() {
    const theme = this.store?.theme ?? "dark";
    const s = this.settings;
    return html`
      <section class="panel" aria-labelledby="prefs-h">
        <h2 id="prefs-h">Preferences</h2>

        <div class="field">
          <span class="field-label">Theme</span>
          <div class="row" role="group" aria-label="Theme">
            <button
              class="fpill ${theme === "dark" ? "active" : ""}"
              aria-pressed=${theme === "dark" ? "true" : "false"}
              @click=${() => this.setTheme("dark")}
            >
              Dark
            </button>
            <button
              class="fpill ${theme === "light" ? "active" : ""}"
              aria-pressed=${theme === "light" ? "true" : "false"}
              @click=${() => this.setTheme("light")}
            >
              Light
            </button>
          </div>
        </div>

        <div class="field">
          <label class="field-label" for="interval">Monitor sampling interval</label>
          <select id="interval" @change=${this.setInterval} .value=${String(s?.monitorIntervalMs ?? 1000)}>
            <option value="1000">1 second</option>
            <option value="2000">2 seconds</option>
            <option value="5000">5 seconds</option>
          </select>
        </div>

        <div class="field">
          <span class="field-label">Watch folders for local games</span>
          ${(s?.watchFolders ?? []).map(
            (f) => html`
              <div class="folder">
                <span>${f}</span>
                <button class="x" @click=${() => this.removeFolder(f)} aria-label="Remove ${f}">✕</button>
              </div>
            `,
          )}
          <div class="row">
            <input
              type="text"
              placeholder="C:\\Games"
              aria-label="Folder path to watch"
              .value=${this.newFolder}
              @input=${(e: Event) => (this.newFolder = (e.target as HTMLInputElement).value)}
              @keydown=${(e: KeyboardEvent) => e.key === "Enter" && this.addFolder()}
            />
            <button class="fpill" @click=${this.addFolder}>Add</button>
          </div>
        </div>

        <div class="field">
          <label class="switch">
            <input
              type="checkbox"
              ?checked=${s?.steamgriddbEnabled ?? false}
              @change=${this.toggleSteamGridDb}
            />
            <span class="track"></span>
            <span>Enable SteamGridDB cover art</span>
          </label>
          <p class="warn">
            Turns on a network connection to fetch richer covers for Epic/local
            games. Off by default — Nirvana is otherwise fully offline. Only takes
            effect in a build compiled with the <code>steamgriddb</code> feature.
          </p>
        </div>
      </section>
    `;
  }

  private renderSupport() {
    return html`
      <section class="panel" aria-labelledby="support-h">
        <h2 id="support-h">Support Nirvana</h2>
        <p>
          Nirvana is free, fully offline, and tracks nothing — no ads, no accounts,
          no telemetry. If it earned a spot on your taskbar, send any amount over
          UPI. It keeps the updates coming.
        </p>
        ${this.donation
          ? html`
              <div class="qr" role="img" aria-label="UPI payment QR code">
                ${unsafeHTML(this.donation.qrSvg)}
              </div>
              <p class="qr-cap">Scan with any UPI app, or use the ID below.</p>
              <div class="upi-row">
                <span class="upi-id" title=${this.donation.upiId}>${this.donation.upiId}</span>
                <button class="pill" @click=${this.copyUpi}>
                  ${this.copied ? "Copied" : "Copy UPI ID"}
                </button>
              </div>
            `
          : html`<p class="result ok" role="status">Loading payment details…</p>`}
      </section>
    `;
  }

  private async copyUpi(): Promise<void> {
    if (!this.donation) return;
    try {
      await navigator.clipboard.writeText(this.donation.upiId);
      this.copied = true;
      clearTimeout(this.#copiedTimer);
      this.#copiedTimer = setTimeout(() => (this.copied = false), 2000);
    } catch {
      this.copied = false;
    }
  }

  private renderPingResult() {
    switch (this.ping.status) {
      case "ok":
        return html`<p class="result ok" role="status">Bridge OK — replied “${this.ping.value}”.</p>`;
      case "error":
        return html`<p class="result error" role="alert">Failed: ${this.ping.message}</p>`;
      default:
        return nothing;
    }
  }
}

declare global {
  interface HTMLElementTagNameMap {
    "settings-view": SettingsView;
  }
}
