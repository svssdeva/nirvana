import { LitElement, html, css, nothing, type PropertyValues } from "lit";
import { customElement, property, state } from "lit/decorators.js";
import type { Game, Source } from "../../ipc";
import {
  coverSrc,
  getCover,
  launchGame,
  setCover,
  setFavorite,
  setTags,
  sources,
  toAppError,
  openInstallFolder,
  uninstallGame,
  listCollections,
  gameCollections,
  setGameCollections,
} from "../../ipc";
import { formatBytes, tagHue } from "../../format";
// Side-effect import registers the <context-menu> custom element. Required: the
// `ContextMenu` symbol below is used only in type position, so a type-only import
// would be elided and `customElements.define` would never run (right-click would
// then create a dead element and silently do nothing).
import "../context-menu";
import type { ContextMenu, MenuItem } from "../context-menu";

const SOURCE_LABEL: Record<Source, string> = {
  steam: "Steam",
  epic: "Epic",
  local: "Local",
  gog: "GOG",
};

// ── Shared singleton context menu ────────────────────────────────────────────
// One <context-menu> is appended to document.body and reused across all tiles.
let menuEl: ContextMenu | undefined;
function menu(): ContextMenu {
  if (!menuEl) {
    menuEl = document.createElement("context-menu") as ContextMenu;
    document.body.appendChild(menuEl);
  }
  return menuEl;
}

/**
 * A single game thumbnail — the `game-tile` from design.md §Components: a
 * fixed-dark `surface-dark-elevated` card (game tiles are dark imagery cards in
 * both canvas modes, so this element intentionally does NOT theme), 16:9 cover at
 * `rounded.md`, source `badge-info` pill top-left, name + size overlaid
 * bottom-left in `body-sm`. Cover art lands in Task 10; until then a placeholder
 * glyph fills the frame.
 */
@customElement("game-tile")
export class GameTile extends LitElement {
  static styles = css`
    :host {
      display: block;
    }
    .tile {
      /* The whole tile is the launch control (a real button for keyboard + SR). */
      position: relative;
      display: block;
      width: 100%;
      padding: 0;
      aspect-ratio: 16 / 9;
      /* Raw dark tokens, not themed --surface*: a game tile is a dark imagery
         card on either canvas, and the white overlay text must stay legible. */
      background: var(--surface-dark-elevated);
      border: none;
      border-radius: var(--rounded-md);
      overflow: hidden;
      cursor: pointer;
      text-align: left;
      color: inherit;
      font: inherit;
      /* design.md elevation: flat at rest; lifts only on press. */
      transition:
        transform 0.08s ease,
        box-shadow 0.08s ease;
    }
    .tile:active {
      transform: scale(0.985);
      box-shadow: 0 4px 12px rgba(0, 0, 0, 0.16);
    }
    .tile:focus-visible {
      outline: 2px solid var(--primary);
      outline-offset: 2px;
    }
    .tile[aria-busy="true"] {
      cursor: progress;
    }
    .status {
      position: absolute;
      inset: 0;
      display: grid;
      place-items: center;
      background: rgba(0, 0, 0, 0.55);
      color: #ffffff;
      font-size: 14px;
      font-weight: 500;
      text-align: center;
      padding: 8px;
    }
    .status.error {
      background: rgba(0, 0, 0, 0.7);
      color: #ff6b6b;
    }
    @media (prefers-reduced-motion: reduce) {
      .tile {
        transition: none;
      }
    }
    img.cover {
      width: 100%;
      height: 100%;
      object-fit: cover;
      display: block;
    }
    img.cover.icon {
      /* Small extracted icons: contain + center on the dark card, don't stretch. */
      object-fit: contain;
      width: 56%;
      height: 56%;
      margin: auto;
    }
    .placeholder {
      width: 100%;
      height: 100%;
      display: grid;
      place-items: center;
      font-size: 35px;
      color: rgba(255, 255, 255, 0.32);
    }
    .badge {
      position: absolute;
      top: 8px;
      left: 8px;
      background: var(--primary);
      color: var(--on-primary);
      font-size: 12px;
      font-weight: 500;
      line-height: 1.5;
      padding: 4px 10px;
      border-radius: var(--rounded-full);
    }
    .meta {
      position: absolute;
      inset: auto 0 0 0;
      padding: 12px;
      display: flex;
      flex-direction: column;
      gap: 2px;
      /* Legibility scrim over the artwork (not a decorative chrome gradient). */
      background: linear-gradient(to top, rgba(0, 0, 0, 0.72), transparent);
    }
    .name {
      /* design.md game-tile: title overlaid in body-sm (16px / 400). */
      font-size: 16px;
      font-weight: 400;
      line-height: 1.5;
      color: #ffffff;
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
    }
    .size {
      font-size: 12px;
      font-weight: 500;
      line-height: 1.5;
      color: rgba(255, 255, 255, 0.8);
    }
    .wrap {
      position: relative;
    }
    /* Favorite star — a separate control (can't nest a button in the tile button). */
    .fav {
      position: absolute;
      top: 8px;
      right: 8px;
      width: 32px;
      height: 32px;
      display: grid;
      place-items: center;
      border: none;
      border-radius: var(--rounded-full);
      background: rgba(0, 0, 0, 0.45);
      color: rgba(255, 255, 255, 0.7);
      font-size: 16px;
      cursor: pointer;
    }
    .fav.on {
      color: #ffce21; /* PS Plus gold accent, reused for the favorite star */
    }
    .fav:focus-visible {
      outline: 2px solid var(--on-primary);
      outline-offset: 2px;
    }
    .tags {
      display: flex;
      flex-wrap: wrap;
      align-items: center;
      gap: 6px;
      margin-top: 8px;
    }
    .chip {
      font: 500 12px/1.5 var(--font-body);
      /* Per-tag color via --h (theme-agnostic translucent tint + bright text). */
      color: hsl(var(--h) 70% 62%);
      background: hsla(var(--h) 65% 50% / 0.16);
      border: 1px solid hsla(var(--h) 65% 50% / 0.4);
      border-radius: var(--rounded-full);
      padding: 2px 10px;
      cursor: pointer;
    }
    .chip:focus-visible {
      outline: 2px solid var(--primary);
      outline-offset: 2px;
    }
    .tag-input {
      width: 100%;
      margin-top: 8px;
      padding: 6px 10px;
      font: 400 14px/1.5 var(--font-body);
      color: var(--on-surface);
      background: var(--bg);
      border: 1px solid var(--primary);
      border-radius: var(--rounded-sm);
    }

    /* ── Compact list row (density="compact") ───────────────────────────── */
    .lrow {
      display: flex;
      align-items: center;
      gap: 12px;
      padding: 8px 12px;
      background: var(--surface);
      border: 1px solid var(--hairline);
      border-radius: var(--rounded-md);
    }
    .lmain {
      flex: 1 1 auto;
      min-width: 0;
      display: flex;
      align-items: center;
      gap: 12px;
      border: none;
      background: none;
      color: inherit;
      font: inherit;
      text-align: left;
      cursor: pointer;
      padding: 0;
    }
    .lmain:focus-visible {
      outline: 2px solid var(--primary);
      outline-offset: 2px;
      border-radius: var(--rounded-sm);
    }
    .lthumb {
      flex: none;
      width: 112px;
      aspect-ratio: 16 / 9;
      border-radius: var(--rounded-sm);
      overflow: hidden;
      background: var(--surface-dark-elevated);
      display: grid;
      place-items: center;
    }
    .lthumb .placeholder {
      font-size: 20px;
      color: rgba(255, 255, 255, 0.32);
    }
    .lthumb img.cover.icon {
      /* Icons in the list thumbnail: contained + centered, not stretched. */
      object-fit: contain;
      width: 56%;
      height: 56%;
      margin: auto;
    }
    .linfo {
      min-width: 0;
      display: flex;
      flex-direction: column;
      gap: 2px;
    }
    .lname {
      font-size: 16px;
      color: var(--on-surface);
      overflow: hidden;
      text-overflow: ellipsis;
      white-space: nowrap;
    }
    .lmeta {
      display: flex;
      align-items: center;
      flex-wrap: wrap;
      gap: 8px;
      font-size: 13px;
      color: var(--on-surface-muted);
    }
    .lbadge {
      background: var(--primary);
      color: var(--on-primary);
      font-size: 11px;
      font-weight: 600;
      padding: 2px 8px;
      border-radius: var(--rounded-full);
    }
    .lstatus {
      color: var(--on-surface-muted);
    }
    .lstatus.err {
      color: var(--danger);
    }
    .lactions {
      flex: none;
      display: flex;
      align-items: center;
      flex-wrap: wrap;
      justify-content: flex-end;
      gap: 6px;
      max-width: 45%;
    }
    /* In a row the favorite is a normal flex child, not an overlay. */
    .lrow .fav {
      position: static;
      background: transparent;
      color: var(--on-surface-muted);
    }
    .lrow .fav.on {
      color: #ffce21;
    }
  `;

  @property({ attribute: false }) game!: Game;
  /** Compact single-row (list) layout instead of the cover card. */
  @property({ type: Boolean, reflect: true }) list = false;

  /** Resolved asset URL for the cover, or null while loading / on placeholder. */
  @state() private src: string | null = null;
  /** Kind of the resolved cover — drives object-fit treatment in the template. */
  @state() private coverKind: "image" | "icon" | "none" = "none";
  /** True while a launch is in flight (blocks double-launch, shows a status). */
  @state() private launching = false;
  /** Transient launch-failure message, auto-cleared. */
  @state() private launchError: string | null = null;
  /** Whether the inline tag editor is open. */
  @state() private editingTags = false;

  /** source → brand color, loaded once (memoized) for the badge fill. */
  @state() private colors?: Map<Source, string>;

  #errorTimer?: ReturnType<typeof setTimeout>;

  /** Tell the container (library-view) that favorite/tags changed → reload. */
  private notifyChanged(): void {
    this.dispatchEvent(new CustomEvent("library-changed", { bubbles: true, composed: true }));
  }

  private async toggleFavorite(): Promise<void> {
    try {
      await setFavorite(this.game.id, !this.game.favorite);
      this.notifyChanged();
    } catch {
      // best-effort; a failed toggle just doesn't persist
    }
  }

  private async commitTags(event: Event): Promise<void> {
    const value = (event.target as HTMLInputElement).value;
    const tags = value
      .split(",")
      .map((t) => t.trim())
      .filter(Boolean);
    this.editingTags = false;
    try {
      await setTags(this.game.id, tags);
      this.notifyChanged();
    } catch {
      // best-effort
    }
  }

  private async setCustomCover(): Promise<void> {
    try {
      const path = await setCover(this.game.id);
      // Cache-bust so the new image shows even if it reused the same filename.
      if (path) this.src = `${coverSrc(path)}?v=${Date.now()}`;
    } catch {
      // best-effort; keep the current cover
    }
  }

  private filterByTag(tag: string): void {
    this.dispatchEvent(
      new CustomEvent("filter-tag", { detail: tag, bubbles: true, composed: true }),
    );
  }

  private onTagKey(event: KeyboardEvent): void {
    if (event.key === "Enter") (event.target as HTMLInputElement).blur();
    else if (event.key === "Escape") this.editingTags = false;
  }

  override updated(changed: PropertyValues<this>): void {
    // Lazy per-tile cover load (api-contract: get_cover is lazy per tile).
    // Reload whenever the bound game changes (the grid reuses tiles by index).
    if (changed.has("game")) void this.loadCover();
  }

  override connectedCallback(): void {
    super.connectedCallback();
    // Brand colors are a progressive enhancement: if the call fails the badge
    // keeps its default primary fill.
    void sources()
      .then((infos) => {
        this.colors = new Map(infos.map((i) => [i.source, i.color]));
      })
      .catch(() => {});
  }

  override disconnectedCallback(): void {
    super.disconnectedCallback();
    clearTimeout(this.#errorTimer);
  }

  /** Inline badge fill from the source's brand color (empty → CSS default). */
  private badgeStyle(source: Source): string {
    const c = this.colors?.get(source);
    return c ? `background:${c}` : "";
  }

  private async launch(): Promise<void> {
    if (this.launching) return;
    this.launching = true;
    this.launchError = null;
    clearTimeout(this.#errorTimer);
    try {
      await launchGame(this.game.id);
    } catch (e) {
      this.launchError = toAppError(e).message;
      // Auto-clear so the tile returns to normal after the user has seen it.
      this.#errorTimer = setTimeout(() => (this.launchError = null), 4000);
    } finally {
      this.launching = false;
    }
  }

  private async loadCover(): Promise<void> {
    const id = this.game.id;
    if (this.game.coverPath) {
      this.src = coverSrc(this.game.coverPath);
      this.coverKind = "image";
      return;
    }
    this.src = null;
    this.coverKind = "none";
    try {
      const ref = await getCover(id);
      // Guard against a stale response after the tile was rebound.
      if (this.game.id !== id) return;
      if (ref.type === "placeholder") {
        this.src = null;
        this.coverKind = "none";
      } else {
        // Both "image" and "icon" have a path — coverSrc works for both.
        this.src = coverSrc(ref.path);
        this.coverKind = ref.type; // "image" | "icon"
      }
    } catch {
      // Keep the placeholder; covers are best-effort.
    }
  }

  // ── Context menu ────────────────────────────────────────────────────────────

  /** Right-click / Shift+F10 handler — prevent default browser menu then open ours. */
  private onContextMenu = (e: MouseEvent): void => {
    e.preventDefault();
    void this.openMenuAt(e.clientX, e.clientY);
  };

  /**
   * PUBLIC so keyboard-nav (Task 9/Shift+F10) can call it without coordinates.
   * Loads collections + this game's memberships, builds the MenuItem tree, then
   * opens the singleton menu.
   */
  async openMenuAt(x: number, y: number): Promise<void> {
    const [cols, mine] = await Promise.all([
      listCollections().catch(() => [] as import("../../ipc").Collection[]),
      gameCollections(this.game.id).catch(() => [] as number[]),
    ]);
    const mineSet = new Set(mine);
    const items: MenuItem[] = [
      { id: "launch", label: "Launch" },
      { id: "folder", label: "Open folder" },
      { id: "favorite", label: this.game.favorite ? "Unfavorite" : "Favorite" },
      { id: "cover", label: "Set cover" },
      { id: "tags", label: "Edit tags" },
      {
        id: "collections",
        label: "Add to collection",
        submenu: [
          ...cols.map((c) => ({ id: `col:${c.id}`, label: c.name, checked: mineSet.has(c.id) })),
          { id: "col:new", label: "New collection…" },
        ],
      },
    ];
    if (this.game.source === "steam") items.push({ id: "uninstall", label: "Uninstall" });
    const m = menu();
    m.removeEventListener("menu-select", this.onMenuSelect);
    m.addEventListener("menu-select", this.onMenuSelect, { once: true } as AddEventListenerOptions);
    m.openAt(x, y, items);
  }

  /** Handles item selection from the context menu. Arrow field for stable `this`. */
  private onMenuSelect = async (e: Event): Promise<void> => {
    const id = (e as CustomEvent<string>).detail;

    if (id === "launch") {
      void this.launch();
    } else if (id === "folder") {
      try {
        await openInstallFolder(this.game.id);
      } catch (err) {
        this.launchError = toAppError(err).message;
        this.#errorTimer = setTimeout(() => (this.launchError = null), 4000);
      }
    } else if (id === "favorite") {
      try {
        await setFavorite(this.game.id, !this.game.favorite);
        this.notifyChanged();
      } catch {
        // best-effort
      }
    } else if (id === "cover") {
      await this.setCustomCover();
      this.notifyChanged();
    } else if (id === "tags") {
      this.editingTags = true;
    } else if (id === "uninstall") {
      try {
        await uninstallGame(this.game.id);
      } catch {
        // best-effort; uninstall opens Steam's own dialog
      }
    } else if (id.startsWith("col:")) {
      if (id === "col:new") {
        this.dispatchEvent(new CustomEvent("goto-settings", { bubbles: true, composed: true }));
      } else {
        try {
          const mine2 = await gameCollections(this.game.id).catch(() => [] as number[]);
          const mineSet2 = new Set(mine2);
          const cid = Number(id.slice(4));
          const next = mineSet2.has(cid) ? mine2.filter((x) => x !== cid) : [...mine2, cid];
          await setGameCollections(this.game.id, next);
          this.notifyChanged();
        } catch {
          // best-effort
        }
      }
    }
  };

  render() {
    return this.list ? this.renderList() : this.renderGrid();
  }

  /** Shared tag chips (used by both layouts). The ✎ and 🖼 buttons are gone — actions live in the context menu now. */
  private renderTagControls(tags: string[]) {
    return html`
      ${tags.map(
        (t) => html`<button
          class="chip"
          style="--h:${tagHue(t)}"
          @click=${() => this.filterByTag(t)}
          title="Filter by ${t}"
        >
          ${t}
        </button>`,
      )}
    `;
  }

  private renderList() {
    const { name, source, sizeBytes, favorite, tags } = this.game;
    const size = formatBytes(sizeBytes);
    const label = `Launch ${name} — ${SOURCE_LABEL[source]}${size ? `, ${size}` : ""}`;
    return html`
      <div class="lrow" @contextmenu=${this.onContextMenu}>
        <button class="lmain" aria-label=${label} aria-busy=${this.launching ? "true" : "false"} @click=${this.launch}>
          <span class="lthumb">
            ${this.src
              ? html`<img class="cover ${this.coverKind === "icon" ? "icon" : ""}" src=${this.src} alt="" />`
              : html`<span class="placeholder" aria-hidden="true">▤</span>`}
          </span>
          <span class="linfo">
            <span class="lname" title=${name}>${name}</span>
            <span class="lmeta">
              <span class="lbadge" style=${this.badgeStyle(source)}>${SOURCE_LABEL[source]}</span>
              ${size ? html`<span class="lsize">${size}</span>` : nothing}
              ${this.launching ? html`<span class="lstatus">Launching…</span>` : nothing}
              ${this.launchError ? html`<span class="lstatus err">${this.launchError}</span>` : nothing}
            </span>
          </span>
        </button>
        <button
          class="fav ${favorite ? "on" : ""}"
          @click=${this.toggleFavorite}
          aria-pressed=${favorite ? "true" : "false"}
          aria-label=${favorite ? `Unfavorite ${name}` : `Favorite ${name}`}
          title=${favorite ? "Unfavorite" : "Favorite"}
        >
          ${favorite ? "★" : "☆"}
        </button>
        <div class="lactions">${this.renderTagControls(tags)}</div>
      </div>
      ${this.editingTags
        ? html`<input
            class="tag-input"
            .value=${tags.join(", ")}
            placeholder="comma, separated, tags"
            autofocus
            aria-label="Tags for ${name}"
            @keydown=${this.onTagKey}
            @blur=${this.commitTags}
          />`
        : nothing}
    `;
  }

  private renderGrid() {
    const { name, source, sizeBytes, favorite, tags } = this.game;
    const size = formatBytes(sizeBytes);
    const label = `Launch ${name} — ${SOURCE_LABEL[source]}${size ? `, ${size}` : ""}`;
    return html`
      <div class="wrap" @contextmenu=${this.onContextMenu}>
        <button
          class="tile"
          aria-label=${label}
          aria-busy=${this.launching ? "true" : "false"}
          @click=${this.launch}
        >
          <span class="badge" style=${this.badgeStyle(source)}>${SOURCE_LABEL[source]}</span>
          ${this.src
            ? html`<img class="cover ${this.coverKind === "icon" ? "icon" : ""}" src=${this.src} alt="" />`
            : html`<div class="placeholder" aria-hidden="true">▤</div>`}
          <div class="meta">
            <span class="name" title=${name}>${name}</span>
            ${size ? html`<span class="size">${size}</span>` : nothing}
          </div>
          ${this.launching
            ? html`<div class="status" role="status">Launching…</div>`
            : nothing}
          ${this.launchError
            ? html`<div class="status error" role="alert">${this.launchError}</div>`
            : nothing}
        </button>
        <button
          class="fav ${favorite ? "on" : ""}"
          @click=${this.toggleFavorite}
          aria-pressed=${favorite ? "true" : "false"}
          aria-label=${favorite ? `Unfavorite ${name}` : `Favorite ${name}`}
          title=${favorite ? "Unfavorite" : "Favorite"}
        >
          ${favorite ? "★" : "☆"}
        </button>
        <div class="tags">${this.renderTagControls(tags)}</div>
        ${this.editingTags
          ? html`<input
              class="tag-input"
              .value=${tags.join(", ")}
              placeholder="comma, separated, tags"
              autofocus
              aria-label="Tags for ${name}"
              @keydown=${this.onTagKey}
              @blur=${this.commitTags}
            />`
          : nothing}
      </div>
    `;
  }
}

declare global {
  interface HTMLElementTagNameMap {
    "game-tile": GameTile;
  }
}
