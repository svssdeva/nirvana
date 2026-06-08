import { LitElement, html, css } from "lit";
import { customElement, property, state } from "lit/decorators.js";
import type { Game } from "../../ipc";
import { nextIndex } from "./grid-nav";
import "./game-tile";

export type Density = "comfortable" | "compact";

type TileElement = HTMLElement & { openMenuAt(x: number, y: number): void };

/**
 * Layout of [`game-tile`]s. `density="comfortable"` is the responsive cover grid
 * (auto-fill ≈ 240px tiles). `density="compact"` is a single game per row (list
 * view) — tiles render their compact row layout. Pure presentation: it renders
 * the `games` it's given; the container (`library-view`) owns fetching/scanning.
 *
 * Keyboard navigation (Task 9): roving tabindex on each <li>; arrow keys move
 * focus, Enter/Space launch, Shift+F10/ContextMenu opens the context menu.
 */
@customElement("game-grid")
export class GameGrid extends LitElement {
  static styles = css`
    :host {
      display: block;
    }
    ul {
      list-style: none;
      margin: 0;
      padding: 0;
      display: grid;
      gap: 24px;
      /* Comfortable: 5 cards per row, stepping down on narrower widths. */
      grid-template-columns: repeat(5, 1fr);
    }
    @media (max-width: 1400px) {
      :host(:not([density="compact"])) ul {
        grid-template-columns: repeat(4, 1fr);
      }
    }
    @media (max-width: 1100px) {
      :host(:not([density="compact"])) ul {
        grid-template-columns: repeat(3, 1fr);
      }
    }
    @media (max-width: 760px) {
      :host(:not([density="compact"])) ul {
        grid-template-columns: repeat(2, 1fr);
      }
    }
    @media (max-width: 480px) {
      :host(:not([density="compact"])) ul {
        grid-template-columns: 1fr;
      }
    }
    /* Compact = a single game per row (list view), not tiny tiles. */
    :host([density="compact"]) ul {
      gap: 8px;
      grid-template-columns: 1fr;
    }
    li:focus {
      outline: none;
    }
    li:focus-visible {
      outline: 2px solid var(--primary);
      outline-offset: 2px;
      border-radius: var(--rounded-md);
    }
  `;

  @property({ attribute: false }) games: Game[] = [];
  /** Tile size; reflected to the host so CSS can switch on it. */
  @property({ reflect: true }) density: Density = "comfortable";

  @state() private focusIndex = 0;

  override updated(changed: Map<PropertyKey, unknown>): void {
    if (changed.has("games")) {
      // Clamp focusIndex to valid range when game list changes.
      if (this.games.length === 0) {
        this.focusIndex = 0;
      } else if (this.focusIndex >= this.games.length) {
        this.focusIndex = this.games.length - 1;
      }
    }
  }

  /** Number of CSS grid columns, derived from computed style at runtime. */
  private columnCount(): number {
    const ul = this.renderRoot.querySelector("ul");
    if (!ul) return 1;
    const cols = getComputedStyle(ul).gridTemplateColumns.split(" ").filter(Boolean).length;
    return cols > 0 ? cols : 1;
  }

  /** Returns the game-tile element at position i (with the openMenuAt signature). */
  private tileAt(i: number): TileElement | undefined {
    return this.renderRoot.querySelectorAll("game-tile")[i] as TileElement | undefined;
  }

  /** Focus the <li> at index i after a Lit update cycle. */
  private moveFocus(i: number): void {
    // Wait for Lit to render the updated tabindex before focusing.
    void this.updateComplete.then(() => {
      const li = this.renderRoot.querySelectorAll("li")[i] as HTMLElement | undefined;
      li?.focus();
    });
  }

  private onKey = (e: KeyboardEvent): void => {
    const { key, shiftKey } = e;

    // Navigation keys
    if (["ArrowRight", "ArrowLeft", "ArrowDown", "ArrowUp", "Home", "End"].includes(key)) {
      const cols = this.density === "compact" ? 1 : this.columnCount();
      const next = nextIndex(this.focusIndex, key, cols, this.games.length);
      if (next !== this.focusIndex) {
        e.preventDefault();
        this.focusIndex = next;
        this.moveFocus(next);
      }
      return;
    }

    // Launch focused tile
    if (key === "Enter" || key === " ") {
      e.preventDefault();
      const tile = this.tileAt(this.focusIndex);
      tile?.shadowRoot
        ?.querySelector<HTMLButtonElement>("button.tile, button.lmain")
        ?.click();
      return;
    }

    // Context menu: Shift+F10 or ContextMenu key
    if ((key === "F10" && shiftKey) || key === "ContextMenu") {
      e.preventDefault();
      const tile = this.tileAt(this.focusIndex);
      if (tile) {
        const rect = tile.getBoundingClientRect();
        void tile.openMenuAt(rect.left + 8, rect.top + 8);
      }
      return;
    }
  };

  private onFocusIn = (e: FocusEvent): void => {
    // When a child li gains focus (e.g. via mouse click), sync focusIndex.
    const target = e.target as HTMLElement;
    const lis = Array.from(this.renderRoot.querySelectorAll<HTMLElement>("li"));
    const idx = lis.indexOf(target);
    if (idx !== -1 && idx !== this.focusIndex) {
      this.focusIndex = idx;
    }
  };

  render() {
    const list = this.density === "compact";
    return html`
      <ul role="list" @keydown=${this.onKey} @focusin=${this.onFocusIn}>
        ${this.games.map(
          (game, i) => html`
            <li role="listitem" tabindex=${i === this.focusIndex ? 0 : -1}>
              <game-tile .game=${game} ?list=${list}></game-tile>
            </li>
          `,
        )}
      </ul>
    `;
  }
}

declare global {
  interface HTMLElementTagNameMap {
    "game-grid": GameGrid;
  }
}
