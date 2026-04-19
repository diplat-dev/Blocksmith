import type { DashboardSnapshot } from "../../types/api";

interface DashboardViewProps {
  snapshot?: DashboardSnapshot;
}

const quickStart = [
  "Set microsoft_client_id and an optional Java override in Settings.",
  "Create or import a profile, or install a Modrinth modpack into a new profile.",
  "Choose an account, preview launch details, and start from Profiles.",
  "Use Discover, Updates, Skins, and Share to keep the library current.",
];

const sections = [
  {
    label: "Launch",
    title: "Profiles and startup",
    body:
      "Create vanilla or Fabric profiles, review the selected version, and keep launch history per profile.",
  },
  {
    label: "Content",
    title: "Discover and install",
    body:
      "Search Modrinth, import .mrpack files, and manage installed content from the same library.",
  },
  {
    label: "Accounts",
    title: "Accounts and skins",
    body:
      "Store offline or Microsoft accounts, bind them to profiles, and apply skins from the local library.",
  },
  {
    label: "Share",
    title: "Export and restore",
    body:
      "Export share codes or manifest files, then rebuild a profile on another machine with the same references.",
  },
];

export function DashboardView({ snapshot }: DashboardViewProps) {
  return (
    <div className="dashboard-grid">
      <section className="panel hero-panel">
        <div className="hero-copy">
          <p className="eyebrow">Dashboard</p>
          <h3>Your launcher at a glance</h3>
          <p>
            Manage profiles, accounts, content, and launch settings without
            leaving the app.
          </p>
        </div>
        <div className="hero-glance">
          <span className="glance-label">Latest profile</span>
          <strong>{snapshot?.latestProfileName ?? "No profiles yet"}</strong>
          <span className="muted">
            Pending updates: {snapshot?.pendingUpdateCount ?? 0}
          </span>
        </div>
      </section>

      <section className="panel stat-panel">
        <p className="eyebrow">Snapshot</p>
        <div className="stats-grid">
          <div>
            <span className="stat-value">{snapshot?.profileCount ?? 0}</span>
            <span className="stat-label">Profiles</span>
          </div>
          <div>
            <span className="stat-value">{snapshot?.fabricProfileCount ?? 0}</span>
            <span className="stat-label">Fabric</span>
          </div>
          <div>
            <span className="stat-value">{snapshot?.vanillaProfileCount ?? 0}</span>
            <span className="stat-label">Vanilla</span>
          </div>
          <div>
            <span className="stat-value">
              {snapshot?.signedInAccountCount ?? 0}
            </span>
            <span className="stat-label">Accounts</span>
          </div>
        </div>
      </section>

      <section className="panel checklist-panel">
        <p className="eyebrow">Get Started</p>
        <ol className="checklist">
          {quickStart.map((step) => (
            <li key={step}>{step}</li>
          ))}
        </ol>
      </section>

      {sections.map((card) => (
        <section className="panel narrative-panel" key={card.label}>
          <p className="eyebrow">{card.label}</p>
          <h3>{card.title}</h3>
          <p>{card.body}</p>
        </section>
      ))}

      <section className="panel latest-panel">
        <p className="eyebrow">Library</p>
        <h3>{snapshot?.latestProfileName ?? "No profiles created yet"}</h3>
        <p>
          Skins in library: <strong>{snapshot?.localSkinCount ?? 0}</strong>
        </p>
        <p>
          Available updates: <strong>{snapshot?.pendingUpdateCount ?? 0}</strong>
        </p>
      </section>
    </div>
  );
}
