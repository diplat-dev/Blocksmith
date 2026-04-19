import { useEffect, useRef, useState } from "react";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import {
  applyInstallPlan,
  createInstallPlan,
  importMrpack,
  installModrinthModpack,
  searchModrinth,
} from "../../lib/tauri";
import type {
  ContentSearchResult,
  ContentType,
  InstallPlan,
  ProfileSummary,
} from "../../types/api";
import { EmptyState } from "../shared/EmptyState";

interface DiscoverViewProps {
  profiles: ProfileSummary[];
}

const contentTypeOptions: Array<{ value: ContentType; label: string }> = [
  { value: "mod", label: "Mods" },
  { value: "resource_pack", label: "Resource Packs" },
  { value: "shader_pack", label: "Shader Packs" },
  { value: "datapack", label: "Datapacks" },
  { value: "modpack", label: "Modpacks" },
];

function labelForContentType(contentType: ContentType): string {
  return contentType.replaceAll("_", " ");
}

export function DiscoverView({ profiles }: DiscoverViewProps) {
  const queryClient = useQueryClient();
  const installPlanRef = useRef<HTMLElement | null>(null);
  const [query, setQuery] = useState("");
  const [results, setResults] = useState<ContentSearchResult[]>([]);
  const [selectedProfileId, setSelectedProfileId] = useState<string>("");
  const [selectedContentType, setSelectedContentType] =
    useState<ContentType>("mod");
  const [datapackWorld, setDatapackWorld] = useState("");
  const [pendingPlan, setPendingPlan] = useState<InstallPlan | null>(null);
  const [modpackName, setModpackName] = useState("");
  const [mrpackPath, setMrpackPath] = useState("");
  const [mrpackName, setMrpackName] = useState("");

  useEffect(() => {
    if (!selectedProfileId && profiles[0]) {
      setSelectedProfileId(profiles[0].id);
    }
  }, [profiles, selectedProfileId]);

  useEffect(() => {
    if (pendingPlan) {
      installPlanRef.current?.scrollIntoView({
        behavior: "smooth",
        block: "start",
      });
    }
  }, [pendingPlan]);

  const refresh = async () => {
    await Promise.all([
      queryClient.invalidateQueries({ queryKey: ["profiles"] }),
      queryClient.invalidateQueries({ queryKey: ["installed-content"] }),
      queryClient.invalidateQueries({ queryKey: ["updates"] }),
      queryClient.invalidateQueries({ queryKey: ["dashboard"] }),
      queryClient.invalidateQueries({ queryKey: ["launch-history"] }),
    ]);
  };

  const searchMutation = useMutation({
    mutationFn: () =>
      searchModrinth({
        query,
        profileId:
          selectedContentType === "modpack" ? null : selectedProfileId || null,
        contentType: selectedContentType,
      }),
    onSuccess: (data) => {
      setResults(data);
      setPendingPlan(null);
    },
  });

  const planMutation = useMutation({
    mutationFn: (result: ContentSearchResult) =>
      createInstallPlan({
        profileId: selectedProfileId,
        projectId: result.projectId,
        contentType: result.contentType,
        installScope: result.contentType === "datapack" ? "world" : "profile",
        targetRelPath:
          result.contentType === "datapack" && datapackWorld.trim()
            ? datapackWorld.trim()
            : null,
      }),
    onSuccess: setPendingPlan,
    onError: () => {
      setPendingPlan(null);
    },
  });

  const installMutation = useMutation({
    mutationFn: (plan: InstallPlan) => applyInstallPlan(plan),
    onSuccess: async () => {
      setPendingPlan(null);
      await refresh();
    },
  });

  const modpackMutation = useMutation({
    mutationFn: (projectId: string) =>
      installModrinthModpack({
        projectId,
        newName: modpackName.trim() || null,
      }),
    onSuccess: async () => {
      setModpackName("");
      await refresh();
    },
  });

  const mrpackImportMutation = useMutation({
    mutationFn: () =>
      importMrpack({
        sourcePath: mrpackPath,
        newName: mrpackName.trim() || null,
      }),
    onSuccess: async () => {
      setMrpackPath("");
      setMrpackName("");
      await refresh();
    },
  });

  const needsProfile = selectedContentType !== "modpack";
  if (profiles.length === 0 && needsProfile) {
    return (
      <EmptyState
        eyebrow="Discover"
        title="Create a profile first"
        body="Profile installs need a target profile. You can still import a local .mrpack file at any time."
      />
    );
  }

  return (
    <div className="stack-grid">
      <div className="profiles-grid secondary">
        <section className="panel">
          <div className="panel-heading">
            <div>
              <p className="eyebrow">Browse</p>
              <h3>Modrinth search</h3>
            </div>
          </div>

          <div className="toolbar-grid">
            {needsProfile ? (
              <label>
                <span>Profile</span>
                <select
                  value={selectedProfileId}
                  onChange={(event) => setSelectedProfileId(event.target.value)}
                >
                  {profiles.map((profile) => (
                    <option key={profile.id} value={profile.id}>
                      {profile.name} ({profile.minecraftVersion})
                    </option>
                  ))}
                </select>
              </label>
            ) : (
              <label>
                <span>New profile name</span>
                <input
                  value={modpackName}
                  onChange={(event) => setModpackName(event.target.value)}
                  placeholder="Optional imported pack name"
                />
              </label>
            )}

            <label>
              <span>Content type</span>
              <select
                value={selectedContentType}
                onChange={(event) =>
                  setSelectedContentType(event.target.value as ContentType)
                }
              >
                {contentTypeOptions.map((option) => (
                  <option key={option.value} value={option.value}>
                    {option.label}
                  </option>
                ))}
              </select>
            </label>

            {selectedContentType === "datapack" ? (
              <label>
                <span>World</span>
                <input
                  value={datapackWorld}
                  onChange={(event) => setDatapackWorld(event.target.value)}
                  placeholder="World name or minecraft/saves/<world>/datapacks"
                />
              </label>
            ) : null}

            <label className="span-2">
              <span>Search query</span>
              <div className="inline-input-row">
                <input
                  value={query}
                  onChange={(event) => setQuery(event.target.value)}
                  placeholder={
                    selectedContentType === "modpack"
                      ? "cobblemon, fabulously optimized, create arcane engineering"
                      : "sodium, iris, continuity, fresh animations"
                  }
                />
                <button
                  className="primary"
                  disabled={!query.trim()}
                  onClick={() => searchMutation.mutate()}
                >
                  {searchMutation.isPending ? "Searching..." : "Search"}
                </button>
              </div>
            </label>
          </div>

          {selectedContentType === "modpack" ? (
            <p className="muted">
              Modpack results create a new profile from the pack's metadata,
              downloads, and overrides.
            </p>
          ) : null}

          {searchMutation.error ? (
            <p className="error-text">{String(searchMutation.error)}</p>
          ) : null}
        </section>

        <section className="panel">
          <div className="panel-heading">
            <div>
              <p className="eyebrow">Import</p>
              <h3>Local `.mrpack` file</h3>
            </div>
          </div>

          <div className="toolbar-grid">
            <label className="span-2">
              <span>Source path</span>
              <input
                value={mrpackPath}
                onChange={(event) => setMrpackPath(event.target.value)}
                placeholder="C:\\Path\\To\\my-pack.mrpack"
              />
            </label>

            <label>
              <span>Name override</span>
              <input
                value={mrpackName}
                onChange={(event) => setMrpackName(event.target.value)}
                placeholder="Optional imported profile name"
              />
            </label>
          </div>

          <div className="panel-actions">
            <button
              className="primary"
              disabled={!mrpackPath.trim()}
              onClick={() => mrpackImportMutation.mutate()}
            >
              {mrpackImportMutation.isPending
                ? "Importing..."
                : "Import `.mrpack`"}
            </button>
            <span className="muted">
              Supports Modrinth format v1 with vanilla or Fabric dependencies.
            </span>
          </div>

          {mrpackImportMutation.error ? (
            <p className="error-text">{String(mrpackImportMutation.error)}</p>
          ) : null}
        </section>
      </div>

      {pendingPlan ? (
        <section className="panel" ref={installPlanRef}>
          <div className="panel-heading">
            <div>
              <p className="eyebrow">Install</p>
              <h3>
                {pendingPlan.projectTitle} / {pendingPlan.versionLabel}
              </h3>
            </div>
          </div>

          <p className="muted">
            Review the target path, warnings, and dependencies before installing.
          </p>

          <div className="detail-grid">
            <div>
              <strong>Target</strong>
              <p className="mono">{pendingPlan.targetPath}</p>
            </div>
            <div>
              <strong>Dependencies</strong>
              <p>{pendingPlan.dependencies.length}</p>
            </div>
          </div>

          {pendingPlan.compatibilityWarnings.length > 0 ? (
            <div className="warning-box">
              {pendingPlan.compatibilityWarnings.map((warning) => (
                <p key={warning}>{warning}</p>
              ))}
            </div>
          ) : null}

          {pendingPlan.dependencies.length > 0 ? (
            <div className="dependency-list">
              {pendingPlan.dependencies.map((dependency) => (
                <div key={`${dependency.projectId}:${dependency.versionId ?? "none"}`}>
                  <strong>{dependency.kind}</strong>
                  <p>{dependency.projectId}</p>
                </div>
              ))}
            </div>
          ) : null}

          <div className="panel-actions">
            <button onClick={() => setPendingPlan(null)}>Close review</button>
            <button
              className="primary"
              onClick={() => installMutation.mutate(pendingPlan)}
            >
              {installMutation.isPending ? "Installing..." : "Install"}
            </button>
          </div>

          {installMutation.error ? (
            <p className="error-text">{String(installMutation.error)}</p>
          ) : null}
        </section>
      ) : null}

      <section className="panel">
        <div className="panel-heading">
          <div>
            <p className="eyebrow">Results</p>
            <h3>Search results</h3>
          </div>
          <span className="status-pill neutral">{results.length} results</span>
        </div>

        {results.length > 0 ? (
          <div className="card-list">
            {results.map((result) => (
              <article className="result-card" key={result.projectId}>
                <div className="result-card-header">
                  <div>
                    <h4>{result.title}</h4>
                    <p className="muted">{result.summary}</p>
                  </div>
                  <span className="chip fabric">
                    {labelForContentType(result.contentType)}
                  </span>
                </div>
                <div className="result-meta">
                  <span>
                    Versions:{" "}
                    {result.supportedVersions.slice(0, 4).join(", ") || "Unknown"}
                  </span>
                  <span>
                    Loaders: {result.supportedLoaders.join(", ") || "Any"}
                  </span>
                </div>
                {result.contentType === "modpack" ? (
                  <button onClick={() => modpackMutation.mutate(result.projectId)}>
                    {modpackMutation.isPending
                      ? "Creating..."
                      : "Create profile from pack"}
                  </button>
                ) : (
                  <button onClick={() => planMutation.mutate(result)}>
                    {planMutation.isPending
                      ? "Preparing..."
                      : pendingPlan?.projectId === result.projectId
                        ? "Review shown below"
                        : "Review install"}
                  </button>
                )}
              </article>
            ))}
          </div>
        ) : (
          <EmptyState
            eyebrow="Results"
            title="Search to browse content"
            body="Use Modrinth search with the selected profile to find compatible mods, packs, shaders, and datapacks."
          />
        )}

        {planMutation.error ? (
          <p className="error-text">{String(planMutation.error)}</p>
        ) : null}
        {modpackMutation.error ? (
          <p className="error-text">{String(modpackMutation.error)}</p>
        ) : null}
      </section>
    </div>
  );
}
