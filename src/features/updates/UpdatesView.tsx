import { useEffect, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { applyUpdateCandidate, listUpdateCandidates } from "../../lib/tauri";
import type { ProfileSummary, UpdateCandidate } from "../../types/api";
import { EmptyState } from "../shared/EmptyState";

interface UpdatesViewProps {
  profiles: ProfileSummary[];
}

export function UpdatesView({ profiles }: UpdatesViewProps) {
  const queryClient = useQueryClient();
  const [selectedProfileId, setSelectedProfileId] = useState<string>("");

  useEffect(() => {
    if (!selectedProfileId && profiles[0]) {
      setSelectedProfileId(profiles[0].id);
    }
  }, [profiles, selectedProfileId]);

  const updatesQuery = useQuery({
    queryKey: ["updates", selectedProfileId],
    queryFn: () => listUpdateCandidates(selectedProfileId || null),
    enabled: profiles.length > 0,
  });

  const applyMutation = useMutation({
    mutationFn: (candidate: UpdateCandidate) =>
      applyUpdateCandidate(
        candidate.installedContentId,
        candidate.targetVersionId,
      ),
    onSuccess: async () => {
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: ["updates"] }),
        queryClient.invalidateQueries({ queryKey: ["installed-content"] }),
        queryClient.invalidateQueries({ queryKey: ["dashboard"] }),
      ]);
    },
  });

  if (profiles.length === 0) {
    return (
      <EmptyState
        eyebrow="Updates"
        title="No profiles to check"
        body="Create, import, or install a modpack first. Then Blocksmith can compare installed Modrinth content against newer compatible versions."
      />
    );
  }

  return (
    <section className="panel">
      <div className="panel-heading">
        <div>
          <p className="eyebrow">Updates</p>
          <h3>Available updates</h3>
        </div>
      </div>

      <div className="toolbar-grid">
        <label>
          <span>Profile</span>
          <select
            value={selectedProfileId}
            onChange={(event) => setSelectedProfileId(event.target.value)}
          >
            {profiles.map((profile) => (
              <option key={profile.id} value={profile.id}>
                {profile.name}
              </option>
            ))}
          </select>
        </label>
      </div>

      {updatesQuery.data && updatesQuery.data.length > 0 ? (
        <div className="card-list">
          {updatesQuery.data.map((candidate) => (
            <article className="result-card" key={candidate.installedContentId}>
              <div className="result-card-header">
                <div>
                  <h4>{candidate.projectId}</h4>
                  <p className="muted">
                    {candidate.currentVersionLabel ?? candidate.currentVersionId}
                    {" -> "}
                    {candidate.targetVersionLabel ?? candidate.targetVersionId}
                  </p>
                </div>
                <button onClick={() => applyMutation.mutate(candidate)}>
                  {applyMutation.isPending ? "Installing..." : "Install update"}
                </button>
              </div>
              {candidate.changelog ? (
                <p className="muted clamp-text">{candidate.changelog}</p>
              ) : (
                <p className="muted">No changelog available for this version.</p>
              )}
            </article>
          ))}
        </div>
      ) : (
        <EmptyState
          eyebrow="Updates"
          title="No compatible updates found"
          body="Everything is current, or this profile does not have a newer compatible version available right now."
        />
      )}

      {updatesQuery.error ? (
        <p className="error-text">{String(updatesQuery.error)}</p>
      ) : null}
      {applyMutation.error ? (
        <p className="error-text">{String(applyMutation.error)}</p>
      ) : null}
    </section>
  );
}
