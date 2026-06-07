// Observable app store + dependency-injection seam. The store holds cross-view
// UI state (currently just the active view); it's provided once by `app-root`
// via `@lit/context` and consumed by any descendant. Views attach a
// `StoreController` to re-render when state changes.
//
// Deliberately framework-light: a tiny subscribe/emit core, no external state
// lib. Grows by adding fields to `AppState` + typed mutators on `Store`.

import { createContext } from "@lit/context";
import type { ReactiveController, ReactiveControllerHost } from "lit";

/** The set of top-level views the shell can switch between. */
export type View = "library" | "disk" | "monitor" | "settings";

/** The two surface modes from design.md §Overview (dark/light canvas). */
export type Theme = "light" | "dark";

/** Cross-view UI state. Immutable snapshots; mutate only via `Store` methods. */
export interface AppState {
  readonly view: View;
  readonly theme: Theme;
}

type Listener = () => void;

export class Store {
  #state: AppState;
  #listeners = new Set<Listener>();

  /** `theme` is injected (read from storage by `theme.ts`) so the Store stays
   *  DOM-free; defaults to the brand's dark canvas. */
  constructor(theme: Theme = "dark") {
    this.#state = { view: "library", theme };
  }

  get state(): AppState {
    return this.#state;
  }

  get view(): View {
    return this.#state.view;
  }

  get theme(): Theme {
    return this.#state.theme;
  }

  /** Switch the active view. No-op (no emit) if already current. */
  setView(view: View): void {
    if (view === this.#state.view) return;
    this.#state = { ...this.#state, view };
    this.#emit();
  }

  /** Set the active surface mode. No-op (no emit) if already current. */
  setTheme(theme: Theme): void {
    if (theme === this.#state.theme) return;
    this.#state = { ...this.#state, theme };
    this.#emit();
  }

  /** Flip between dark and light canvas modes. */
  toggleTheme(): void {
    this.setTheme(this.#state.theme === "dark" ? "light" : "dark");
  }

  /** Register a change listener; returns an unsubscribe fn. */
  subscribe(fn: Listener): () => void {
    this.#listeners.add(fn);
    return () => this.#listeners.delete(fn);
  }

  #emit(): void {
    for (const fn of this.#listeners) fn();
  }
}

/** Context token used to inject the single `Store` instance down the tree. */
export const storeContext = createContext<Store>("nirvana-store");

/**
 * Reactive controller bridging `Store` change events to Lit's update cycle:
 * subscribes on host connect, requests a re-render on every emit, unsubscribes
 * on disconnect. Attach in a component: `new StoreController(this, store)`.
 */
export class StoreController implements ReactiveController {
  #unsubscribe?: () => void;

  constructor(
    private host: ReactiveControllerHost,
    private store: Store,
  ) {
    host.addController(this);
  }

  hostConnected(): void {
    this.#unsubscribe = this.store.subscribe(() => this.host.requestUpdate());
  }

  hostDisconnected(): void {
    this.#unsubscribe?.();
    this.#unsubscribe = undefined;
  }
}
