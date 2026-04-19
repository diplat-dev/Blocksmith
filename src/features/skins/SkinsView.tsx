import { useEffect, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  applySkinToAccount,
  deleteSkin,
  importSkin,
  listAccounts,
  listSkins,
} from "../../lib/tauri";
import { EmptyState } from "../shared/EmptyState";

export function SkinsView() {
  const queryClient = useQueryClient();
  const [sourcePath, setSourcePath] = useState("");
  const [displayName, setDisplayName] = useState("");
  const [modelVariant, setModelVariant] = useState<"classic" | "slim">("classic");
  const [tags, setTags] = useState("");
  const [selectedAccountId, setSelectedAccountId] = useState("");

  const skinsQuery = useQuery({
    queryKey: ["skins"],
    queryFn: listSkins,
  });

  const accountsQuery = useQuery({
    queryKey: ["accounts"],
    queryFn: listAccounts,
  });

  useEffect(() => {
    if (!selectedAccountId && accountsQuery.data?.[0]) {
      setSelectedAccountId(accountsQuery.data[0].id);
    }
  }, [accountsQuery.data, selectedAccountId]);

  const importMutation = useMutation({
    mutationFn: () =>
      importSkin({
        sourcePath,
        displayName: displayName || null,
        modelVariant,
        tags: tags
          .split(",")
          .map((tag) => tag.trim())
          .filter(Boolean),
      }),
    onSuccess: async () => {
      setSourcePath("");
      setDisplayName("");
      setTags("");
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: ["skins"] }),
        queryClient.invalidateQueries({ queryKey: ["dashboard"] }),
      ]);
    },
  });

  const deleteMutation = useMutation({
    mutationFn: (skinId: string) => deleteSkin(skinId),
    onSuccess: async () => {
      await Promise.all([
        queryClient.invalidateQueries({ queryKey: ["skins"] }),
        queryClient.invalidateQueries({ queryKey: ["accounts"] }),
        queryClient.invalidateQueries({ queryKey: ["dashboard"] }),
      ]);
    },
  });

  const applyMutation = useMutation({
    mutationFn: (skinId: string) => applySkinToAccount(selectedAccountId, skinId),
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: ["accounts"] });
    },
  });

  return (
    <div className="stack-grid">
      <section className="panel">
        <div className="panel-heading">
          <div>
            <p className="eyebrow">Skins</p>
            <h3>Skin library</h3>
          </div>
        </div>

        <div className="toolbar-grid">
          <label className="span-2">
            <span>PNG path</span>
            <input
              value={sourcePath}
              onChange={(event) => setSourcePath(event.target.value)}
              placeholder="C:\\Path\\To\\my-skin.png"
            />
          </label>

          <label>
            <span>Display name</span>
            <input
              value={displayName}
              onChange={(event) => setDisplayName(event.target.value)}
              placeholder="Red Hoodie"
            />
          </label>

          <label>
            <span>Variant</span>
            <select
              value={modelVariant}
              onChange={(event) =>
                setModelVariant(event.target.value as "classic" | "slim")
              }
            >
              <option value="classic">classic</option>
              <option value="slim">slim</option>
            </select>
          </label>

          <label className="span-2">
            <span>Tags</span>
            <input
              value={tags}
              onChange={(event) => setTags(event.target.value)}
              placeholder="pvp, stream, event"
            />
          </label>
        </div>

        <div className="panel-actions">
          <button
            className="primary"
            disabled={!sourcePath.trim()}
            onClick={() => importMutation.mutate()}
          >
            {importMutation.isPending ? "Importing..." : "Import skin"}
          </button>
        </div>
      </section>

      {(skinsQuery.data ?? []).length > 0 ? (
        <section className="panel">
          <div className="panel-heading">
            <div>
              <p className="eyebrow">Library</p>
              <h3>Stored skins</h3>
            </div>
            <label className="inline-select">
              <span>Apply target</span>
              <select
                value={selectedAccountId}
                onChange={(event) => setSelectedAccountId(event.target.value)}
              >
                <option value="">Choose account</option>
                {(accountsQuery.data ?? []).map((account) => (
                  <option key={account.id} value={account.id}>
                    {account.username}
                  </option>
                ))}
              </select>
            </label>
          </div>

          <div className="card-list">
            {(skinsQuery.data ?? []).map((skin) => (
              <article className="result-card" key={skin.id}>
                <div className="result-card-header">
                  <div>
                    <h4>{skin.displayName}</h4>
                    <p className="muted">{skin.modelVariant}</p>
                  </div>
                  <div className="row-actions">
                    <button
                      onClick={() => applyMutation.mutate(skin.id)}
                      disabled={!selectedAccountId}
                    >
                      Apply
                    </button>
                    <button
                      className="danger-ghost"
                      onClick={() => deleteMutation.mutate(skin.id)}
                    >
                      Remove
                    </button>
                  </div>
                </div>
                <p className="mono">{skin.localFilePath}</p>
                <p className="muted">
                  {skin.tags.length > 0 ? skin.tags.join(", ") : "No tags"}
                </p>
              </article>
            ))}
          </div>
        </section>
      ) : (
        <EmptyState
          eyebrow="Skins"
          title="No skins imported yet"
          body="Point Blocksmith at a local PNG file and it will copy it into the launcher skin library."
        />
      )}

      {skinsQuery.error ? (
        <p className="error-text">{String(skinsQuery.error)}</p>
      ) : null}
      {importMutation.error ? (
        <p className="error-text">{String(importMutation.error)}</p>
      ) : null}
      {deleteMutation.error ? (
        <p className="error-text">{String(deleteMutation.error)}</p>
      ) : null}
      {applyMutation.error ? (
        <p className="error-text">{String(applyMutation.error)}</p>
      ) : null}
    </div>
  );
}
