import { LitElement, html, css, nothing } from "lit";
import { customElement, property, state } from "lit/decorators.js";
import type { Game } from "../../ipc";
import { getHero, coverSrc, launchGame, toAppError } from "../../ipc";

@customElement("hero-banner")
export class HeroBanner extends LitElement {
  static styles = css`
    :host { display: block; }
    .band {
      position: relative;
      min-height: 260px;
      border-radius: var(--rounded-md);
      overflow: hidden;
      display: flex;
      align-items: flex-end;
      background: linear-gradient(120deg, var(--surface-elevated), var(--bg));
    }
    .band.has-art { background: var(--bg); }
    img.art {
      position: absolute; inset: 0;
      width: 100%; height: 100%;
      object-fit: cover;
    }
    .scrim {
      position: absolute; inset: 0;
      background: linear-gradient(90deg, rgba(0,0,0,0.78) 0%, rgba(0,0,0,0.35) 45%, transparent 75%);
    }
    .content { position: relative; padding: 28px; max-width: 60%; }
    .name {
      font-family: var(--font-display);
      font-weight: 300;
      font-size: 40px;
      line-height: 1.1;
      color: #fff;
      margin: 0 0 4px;
    }
    .source { font-size: 13px; color: rgba(255,255,255,0.75); margin: 0 0 16px; }
    .play {
      font: 700 16px/1.25 var(--font-body);
      color: var(--on-primary);
      background: var(--primary);
      border: none; border-radius: var(--rounded-full);
      padding: 10px 26px; cursor: pointer;
    }
    .play:active { background: var(--primary-pressed); }
    .play:focus-visible { outline: 2px solid #fff; outline-offset: 2px; }
    @media (max-width: 768px) {
      .content { max-width: 100%; padding: 20px; }
      .name { font-size: 30px; }
    }
  `;

  @property({ attribute: false }) game!: Game;
  @state() private art: string | null = null;
  @state() private launching = false;

  override updated(changed: Map<string, unknown>): void {
    if (changed.has("game")) void this.loadArt();
  }

  private async loadArt(): Promise<void> {
    this.art = null;
    const id = this.game.id;
    try {
      const ref = await getHero(id);
      // Guard against a stale response if the featured game changed mid-flight.
      if (this.game.id !== id) return;
      if (ref.type === "image") this.art = coverSrc(ref.path);
    } catch {
      /* best-effort: keep the gradient */
    }
  }

  private async play(): Promise<void> {
    if (this.launching) return;
    this.launching = true;
    try {
      await launchGame(this.game.id);
    } catch (e) {
      console.error(toAppError(e).message);
    } finally {
      this.launching = false;
    }
  }

  render() {
    return html`
      <section class="band ${this.art ? "has-art" : ""}" aria-label="Featured: ${this.game.name}">
        ${this.art ? html`<img class="art" src=${this.art} alt="" />` : nothing}
        <div class="scrim"></div>
        <div class="content">
          <h2 class="name">${this.game.name}</h2>
          <p class="source">${this.game.source.toUpperCase()}</p>
          <button class="play" aria-busy=${this.launching ? "true" : "false"} @click=${this.play}>
            ▶ Play
          </button>
        </div>
      </section>
    `;
  }
}

declare global {
  interface HTMLElementTagNameMap { "hero-banner": HeroBanner; }
}
