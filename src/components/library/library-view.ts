import { LitElement, html, css, nothing } from "lit";
import { customElement, state } from "lit/decorators.js";
import type { AppError, Game, LibraryQuery, SortBy, Source, SourceInfo } from "../../ipc";
import { getLibrary, scanLibrary, sources, subscribe, toAppError } from "../../ipc";
import { tagHue } from "../../format";
import type { Density } from "./game-grid";
import "../view-page";
import "../empty-state";
import "./game-grid";
import "./continue-row";
import { recentlyPlayed } from "./continue-row";

const LAYOUT_KEY = "nirvana-layout";

type Status = "loading" | "idle" | "scanning" | "error";

/**
 * Library container (design.md game-tile grid view). Owns data + lifecycle:
 * loads the persisted library on connect, runs a scan on demand, and listens to
 * `scan://progress` for live per-source counts. Presentation is delegated to
 * `game-grid`/`game-tile`; loading/empty/error states are handled here per the
 * frontend-ui-engineering skill (never a blank screen).
 *
 * Shadow DOM (unlike the placeholder views) so the toolbar + scan pill can be
 * styled with the design tokens directly; tokens pierce the shadow boundary.
 */
@customElement("library-view")
export class LibraryView extends LitElement {
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
      font-family: var(--font-body);
      font-size: 18px;
      font-weight: 700;
      letter-spacing: 0.45px;
      line-height: 1.25;
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
    .progress {
      /* caption-md (14px / 400) — metadata, not a control. */
      font-size: 14px;
      font-weight: 400;
      color: var(--on-surface-muted);
    }
    .filters {
      display: flex;
      flex-wrap: wrap;
      align-items: center;
      gap: 8px;
      padding: 0 48px 24px;
    }
    .search {
      flex: 1 1 200px;
      min-width: 0;
      padding: 8px 14px;
      font: 400 16px/1.5 var(--font-body);
      color: var(--on-surface);
      background: var(--bg);
      border: 1px solid var(--hairline);
      border-radius: var(--rounded-full);
    }
    .search:focus-visible {
      outline: none;
      border-color: var(--primary);
    }
    /* design.md filter-pill: compact pill; active lifts to the accent. */
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
    /* Brand-color dot that color-codes each source pill. */
    .sdot {
      display: inline-block;
      width: 8px;
      height: 8px;
      border-radius: var(--rounded-full);
      margin-right: 7px;
      vertical-align: middle;
      /* keep the dot visible on the blue active pill */
      box-shadow: 0 0 0 1px rgba(255, 255, 255, 0.5);
    }
    .fpill:focus-visible {
      outline: 2px solid var(--primary);
      outline-offset: 2px;
    }
    .sort {
      padding: 8px 12px;
      font: 500 14px/1.25 var(--font-body);
      color: var(--on-surface);
      background: var(--surface-elevated);
      border: 1px solid var(--hairline);
      border-radius: var(--rounded-full);
      cursor: pointer;
    }
    .tags-row {
      padding-top: 0;
    }
    .tagpill {
      font: 600 12px/1.5 var(--font-body);
      padding: 4px 12px;
      border-radius: var(--rounded-full);
      cursor: pointer;
      color: hsl(var(--h) 70% 62%);
      background: hsla(var(--h) 65% 50% / 0.16);
      border: 1px solid hsla(var(--h) 65% 50% / 0.4);
    }
    .tagpill.active {
      color: var(--on-primary);
      background: hsl(var(--h) 60% 45%);
      border-color: hsl(var(--h) 60% 45%);
    }
    .tagpill:focus-visible {
      outline: 2px solid var(--primary);
      outline-offset: 2px;
    }
    .body {
      padding: 0 48px 48px;
    }
    .error {
      max-width: 520px;
      background: var(--surface);
      border: 1px solid var(--hairline);
      border-left: 3px solid var(--danger);
      border-radius: var(--rounded-md);
      padding: 24px;
    }
    .error h2 {
      margin: 0 0 8px;
      font-size: 18px;
      font-weight: 600;
      color: var(--on-surface);
    }
    .error p {
      margin: 0;
      font-size: 16px;
      line-height: 1.5;
      color: var(--on-surface-muted);
    }
    .skeleton {
      list-style: none;
      margin: 0;
      padding: 0;
      display: grid;
      gap: 24px;
      grid-template-columns: repeat(4, 1fr);
    }
    .skeleton li {
      aspect-ratio: 16 / 9;
      background: var(--surface-dark-elevated);
      border-radius: var(--rounded-md);
      animation: pulse 1.4s ease-in-out infinite;
    }
    @keyframes pulse {
      0%,
      100% {
        opacity: 1;
      }
      50% {
        opacity: 0.5;
      }
    }
    @media (max-width: 1024px) {
      .skeleton {
        grid-template-columns: repeat(3, 1fr);
      }
    }
    @media (max-width: 768px) {
      .toolbar,
      .body {
        padding-left: 24px;
        padding-right: 24px;
      }
      .skeleton {
        grid-template-columns: repeat(2, 1fr);
      }
    }
  `;

  @state() private games: Game[] = [];
  @state() private status: Status = "loading";
  @state() private error: AppError | null = null;
  @state() private progress = new Map<Source, number>();
  /** Current filter/sort/search; sent to get_library on every change. */
  @state() private query: LibraryQuery = { sort: "name", descending: false };

  /** Tags seen across the library, kept from unfiltered loads so the tag pills
   *  don't vanish once a tag filter narrows the results. */
  @state() private knownTags: string[] = [];
  /** Grid density, persisted in localStorage. */
  @state() private layout: Density = readLayout();
  /** Known stores (id/display/color), for the source filter pills. */
  @state() private sources: SourceInfo[] = [];

  #unlisteners: Array<() => void> = [];
  #searchTimer?: ReturnType<typeof setTimeout>;
  /** A tile changed a favorite/tags → reload to reflect it (and re-filter). */
  #onChanged = () => void this.refresh();
  /** A tile's tag chip was clicked → filter by that tag. */
  #onFilterTag = (e: Event) => this.setTag((e as CustomEvent<string>).detail);

  override connectedCallback(): void {
    super.connectedCallback();
    void this.listenForProgress();
    void this.refresh();
    // Source pills are data-driven so a new store appears (themed) automatically.
    void sources()
      .then((s) => (this.sources = s))
      .catch(() => {});
    this.addEventListener("library-changed", this.#onChanged);
    this.addEventListener("filter-tag", this.#onFilterTag);
  }

  override disconnectedCallback(): void {
    super.disconnectedCallback();
    for (const unlisten of this.#unlisteners) unlisten();
    this.#unlisteners = [];
    this.removeEventListener("library-changed", this.#onChanged);
    this.removeEventListener("filter-tag", this.#onFilterTag);
    clearTimeout(this.#searchTimer);
  }

  private setTag(tag?: string): void {
    // Toggle off if the same tag is clicked again.
    this.query = { ...this.query, tag: this.query.tag === tag ? undefined : tag };
    void this.refresh();
  }

  private setLayout(layout: Density): void {
    this.layout = layout;
    try {
      localStorage.setItem(LAYOUT_KEY, layout);
    } catch {
      // best-effort persistence
    }
  }

  private setSource(source?: Source): void {
    this.query = { ...this.query, source };
    void this.refresh();
  }

  private setSort(sort: SortBy): void {
    // Name reads better ascending; size/recency default to "most first".
    this.query = { ...this.query, sort, descending: sort !== "name" };
    void this.refresh();
  }

  private toggleFavorites(): void {
    this.query = { ...this.query, favoritesOnly: !this.query.favoritesOnly };
    void this.refresh();
  }

  private onSearch(event: Event): void {
    const search = (event.target as HTMLInputElement).value;
    clearTimeout(this.#searchTimer);
    this.#searchTimer = setTimeout(() => {
      this.query = { ...this.query, search };
      void this.refresh();
    }, 150);
  }

  private async listenForProgress(): Promise<void> {
    try {
      const unlisten = await subscribe("scan://progress", (event) => {
        const { source, found } = event.payload;
        this.progress = new Map(this.progress).set(source, found);
      });
      // Guard the connect/disconnect race: if we already detached, tear down now.
      if (this.isConnected) this.#unlisteners.push(unlisten);
      else unlisten();
    } catch {
      // Event bus not ready yet (cold start) — progress counts just won't show;
      // swallow so it isn't an unhandled rejection. Scanning still works.
    }
  }

  /**
   * Load the persisted library with the current query (no scan). Retries once on
   * a transient failure (e.g. the IPC bridge not ready on a cold start) before
   * surfacing an error, so a fresh launch doesn't flash a scary message.
   */
  private async refresh(retry = true): Promise<void> {
    if (this.status !== "scanning") this.status = "loading";
    try {
      this.games = await getLibrary(this.query);
      this.mergeKnownTags();
      if (this.status !== "scanning") this.status = "idle";
    } catch (e) {
      if (retry) {
        await new Promise((r) => setTimeout(r, 300));
        return this.refresh(false);
      }
      this.error = toAppError(e);
      this.status = "error";
    }
  }

  /** Trigger a full scan, then show the freshly persisted library (filtered). */
  private async scan(): Promise<void> {
    this.status = "scanning";
    this.error = null;
    this.progress = new Map();
    try {
      await scanLibrary(true);
      this.games = await getLibrary(this.query);
      this.status = "idle";
    } catch (e) {
      this.error = toAppError(e);
      this.status = "error";
    }
  }

  /** Accumulate tag names seen so the tag pills stay stable across filters. */
  private mergeKnownTags(): void {
    const tags = new Set(this.knownTags);
    for (const g of this.games) for (const t of g.tags) tags.add(t);
    if (tags.size !== this.knownTags.length) {
      this.knownTags = [...tags].sort((a, b) => a.toLowerCase().localeCompare(b.toLowerCase()));
    }
  }

  private hasActiveFilter(): boolean {
    const q = this.query;
    return Boolean(q.search?.trim() || q.source || q.favoritesOnly || q.tag);
  }

  private progressText(): string {
    const parts = [...this.progress].map(
      ([source, found]) => `${source[0].toUpperCase()}${source.slice(1)} ${found}`,
    );
    return parts.length ? `Found ${parts.join(" · ")}` : "Scanning…";
  }

  render() {
    const scanning = this.status === "scanning";
    return html`
      <view-page
        heading="Library"
        tagline="Your Steam, Epic, and local games — discovered and unified."
      >
        <div class="toolbar">
          <button class="scan" @click=${this.scan} ?disabled=${scanning}>
            ${scanning ? "Scanning…" : "Scan library"}
          </button>
          ${scanning
            ? html`<span class="progress" role="status">${this.progressText()}</span>`
            : nothing}
        </div>
        ${this.renderFilters()}
        ${!this.hasActiveFilter() && this.games.length
          ? html`<div class="body" style="padding-bottom:0">
              <continue-row .games=${recentlyPlayed(this.games)}></continue-row>
            </div>`
          : nothing}
        <div class="body">${this.renderBody()}</div>
      </view-page>
    `;
  }

  private renderFilters() {
    const q = this.query;
    // `color` paints a small brand dot before the label; the active pill keeps
    // design.md's blue chip treatment (the dot still identifies the source).
    const sourcePill = (label: string, source?: Source, color?: string) => html`
      <button
        class="fpill ${q.source === source ? "active" : ""}"
        aria-pressed=${q.source === source ? "true" : "false"}
        @click=${() => this.setSource(source)}
      >
        ${color ? html`<span class="sdot" style="background:${color}"></span>` : nothing}${label}
      </button>
    `;
    return html`
      <div class="filters" role="group" aria-label="Filter and sort the library">
        <input
          class="search"
          type="search"
          placeholder="Search games…"
          aria-label="Search games"
          @input=${this.onSearch}
        />
        ${sourcePill("All", undefined)}
        ${this.sources.map((s) => sourcePill(s.display, s.source, s.color))}
        <button
          class="fpill ${q.favoritesOnly ? "active" : ""}"
          aria-pressed=${q.favoritesOnly ? "true" : "false"}
          @click=${this.toggleFavorites}
        >
          ★ Favorites
        </button>
        <select
          class="sort"
          aria-label="Sort by"
          @change=${(e: Event) => this.setSort((e.target as HTMLSelectElement).value as SortBy)}
        >
          <option value="name">Name</option>
          <option value="size">Size</option>
          <option value="lastPlayed">Last played</option>
        </select>
        <select
          class="sort"
          aria-label="Layout"
          .value=${this.layout}
          @change=${(e: Event) => this.setLayout((e.target as HTMLSelectElement).value as Density)}
        >
          <option value="comfortable">Comfortable</option>
          <option value="compact">Compact</option>
        </select>
      </div>
      ${this.knownTags.length
        ? html`<div class="filters tags-row" role="group" aria-label="Filter by tag">
            ${this.knownTags.map(
              (t) => html`<button
                class="tagpill ${q.tag === t ? "active" : ""}"
                style="--h:${tagHue(t)}"
                aria-pressed=${q.tag === t ? "true" : "false"}
                @click=${() => this.setTag(t)}
              >
                ${t}
              </button>`,
            )}
          </div>`
        : nothing}
    `;
  }

  private renderBody() {
    if (this.status === "error") {
      return html`
        <div class="error" role="alert">
          <h2>Couldn't load your library</h2>
          <p>${this.error?.message ?? "Unknown error."} Try scanning again.</p>
        </div>
      `;
    }
    // Initial load, or a scan with nothing on screen yet → skeleton.
    if ((this.status === "loading" || this.status === "scanning") && this.games.length === 0) {
      return html`
        <ul class="skeleton" aria-busy="true" aria-label="Loading library">
          ${Array.from({ length: 8 }, () => html`<li></li>`)}
        </ul>
      `;
    }
    if (this.games.length === 0) {
      // Distinguish a genuinely empty library from a filter that excludes everything.
      return this.hasActiveFilter()
        ? html`<empty-state
            glyph="🔍"
            label="No games match"
            hint="Try clearing the search or filters."
          ></empty-state>`
        : html`<empty-state
            glyph="▤"
            label="No games yet"
            hint="Run a library scan to discover your installed Steam, Epic, and local games."
          ></empty-state>`;
    }
    return html`<game-grid .games=${this.games} density=${this.layout}></game-grid>`;
  }
}

function readLayout(): Density {
  try {
    return localStorage.getItem(LAYOUT_KEY) === "compact" ? "compact" : "comfortable";
  } catch {
    return "comfortable";
  }
}

declare global {
  interface HTMLElementTagNameMap {
    "library-view": LibraryView;
  }
}
