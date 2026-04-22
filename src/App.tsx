import { startTransition, useDeferredValue } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { AccountsView } from "./features/accounts/AccountsView";
import { DashboardView } from "./features/dashboard/DashboardView";
import { DiscoverView } from "./features/discover/DiscoverView";
import { ProfileWorkbench } from "./features/profiles/ProfileWorkbench";
import { SkinsView } from "./features/skins/SkinsView";
import { UpdatesView } from "./features/updates/UpdatesView";
import {
  listProfiles,
  listSettings,
  getDashboardSnapshot,
  upsertSetting,
} from "./lib/tauri";
import { useUiStore, type AppView } from "./state/ui-store";

const navItems: AppView[] = [
  "dashboard",
  "profiles",
  "discover",
  "updates",
  "accounts",
  "skins",
  "settings",
];

function labelForView(view: AppView): string {
  switch (view) {
    case "dashboard":
      return "Dashboard";
    case "profiles":
      return "Profiles";
    case "discover":
      return "Discover";
    case "updates":
      return "Updates";
    case "accounts":
      return "Accounts";
    case "skins":
      return "Skins";
    case "settings":
      return "Settings";
    default:
      return "Workspace";
  }
}

export default function App() {
  const queryClient = useQueryClient();
  const activeView = useUiStore((state) => state.activeView);
  const profileSearch = useUiStore((state) => state.profileSearch);
  const setActiveView = useUiStore((state) => state.setActiveView);
  const setProfileSearch = useUiStore((state) => state.setProfileSearch);
  const deferredSearch = useDeferredValue(profileSearch);

  const dashboardQuery = useQuery({
    queryKey: ["dashboard"],
    queryFn: getDashboardSnapshot,
  });

  const profilesQuery = useQuery({
    queryKey: ["profiles"],
    queryFn: listProfiles,
  });

  const settingsQuery = useQuery({
    queryKey: ["settings"],
    queryFn: listSettings,
  });

  const settingMutation = useMutation({
    mutationFn: ({
      key,
      value,
      category,
    }: {
      key: string;
      value: string;
      category: string;
    }) => upsertSetting(key, value, category),
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: ["settings"] });
    },
  });

  const activeLabel = labelForView(activeView);

  const renderView = () => {
    switch (activeView) {
      case "dashboard":
        return <DashboardView snapshot={dashboardQuery.data} />;
      case "profiles":
        return (
          <ProfileWorkbench
            launcherUnlocked={dashboardQuery.data?.launcherUnlocked ?? false}
            profiles={profilesQuery.data ?? []}
            search={deferredSearch}
          />
        );
      case "settings":
        return (
          <section className="panel settings-panel">
            <div className="panel-heading">
              <div>
                <p className="eyebrow">Global settings</p>
                <h3>Launcher defaults</h3>
              </div>
            </div>

            <div className="settings-grid">
              {(settingsQuery.data ?? []).map((setting) => (
                <label className="setting-row" key={setting.key}>
                  <span>
                    <strong>{setting.key}</strong>
                    <small>{setting.category}</small>
                  </span>
                  <input
                    defaultValue={setting.value}
                    onBlur={(event) =>
                      settingMutation.mutate({
                        key: setting.key,
                        value: event.target.value,
                        category: setting.category,
                      })
                    }
                  />
                </label>
              ))}
            </div>
          </section>
        );
      case "discover":
        return <DiscoverView profiles={profilesQuery.data ?? []} />;
      case "updates":
        return <UpdatesView profiles={profilesQuery.data ?? []} />;
      case "accounts":
        return (
          <AccountsView
            launcherUnlocked={dashboardQuery.data?.launcherUnlocked ?? false}
            profiles={profilesQuery.data ?? []}
          />
        );
      case "skins":
        return <SkinsView />;
      default:
        return null;
    }
  };

  return (
    <div className="app-shell">
      <aside className="sidebar">
        <div className="brand-lockup">
          <p className="brand-mark">Blocksmith</p>
          <h1>Minecraft launcher</h1>
          <p className="muted">
            Profiles, content, accounts, skins, and launch settings in one
            place.
          </p>
        </div>

        <nav className="nav-list">
          {navItems.map((item) => (
            <button
              key={item}
              className={item === activeView ? "nav-item active" : "nav-item"}
              onClick={() =>
                startTransition(() => {
                  setActiveView(item);
                })
              }
            >
              <span>{labelForView(item)}</span>
            </button>
          ))}
        </nav>

        <section className="sidebar-card">
          <p className="eyebrow">Search</p>
          <input
            value={profileSearch}
            onChange={(event) => setProfileSearch(event.target.value)}
            placeholder="Filter profiles by name, type, or version"
          />
        </section>
      </aside>

      <main className="content-area">
        <header className="topbar">
          <div>
            <p className="eyebrow">Section</p>
            <h2>{activeLabel}</h2>
          </div>
          <div className="topbar-stats">
            <span
              className={
                dashboardQuery.data?.launcherUnlocked
                  ? "status-pill ready"
                  : "status-pill neutral"
              }
            >
              {dashboardQuery.data?.launcherUnlocked
                ? "Launch enabled"
                : "Sign in once to launch"}
            </span>
            <span>
              Profiles <strong>{dashboardQuery.data?.profileCount ?? 0}</strong>
            </span>
            <span>
              Accounts{" "}
              <strong>{dashboardQuery.data?.signedInAccountCount ?? 0}</strong>
            </span>
            <span>
              Skins <strong>{dashboardQuery.data?.localSkinCount ?? 0}</strong>
            </span>
            <span>
              Updates <strong>{dashboardQuery.data?.pendingUpdateCount ?? 0}</strong>
            </span>
          </div>
        </header>

        {(dashboardQuery.isLoading ||
          profilesQuery.isLoading ||
          settingsQuery.isLoading) &&
        !dashboardQuery.data &&
        !profilesQuery.data &&
        !settingsQuery.data ? (
          <section className="panel loading-panel">
            <p className="eyebrow">Loading</p>
            <h3>Opening your local Blocksmith library</h3>
          </section>
        ) : (
          renderView()
        )}
      </main>
    </div>
  );
}
