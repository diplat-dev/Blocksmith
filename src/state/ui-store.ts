import { create } from "zustand";

export type AppView =
  | "dashboard"
  | "profiles"
  | "discover"
  | "updates"
  | "accounts"
  | "skins"
  | "settings";

interface UiState {
  activeView: AppView;
  profileSearch: string;
  setActiveView: (view: AppView) => void;
  setProfileSearch: (value: string) => void;
}

export const useUiStore = create<UiState>((set) => ({
  activeView: "dashboard",
  profileSearch: "",
  setActiveView: (activeView) => set({ activeView }),
  setProfileSearch: (profileSearch) => set({ profileSearch }),
}));

