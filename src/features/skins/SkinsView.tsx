import { ChangeEvent, useEffect, useRef, useState } from "react";
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
  const fileInputRef = useRef<HTMLInputElement | null>(null);
  const [sourcePath, setSourcePath] = useState("");
  const [displayName, setDisplayName] = useState("");
  const [modelVariant, setModelVariant] = useState<"classic" | "slim">("classic");
  const [tags, setTags] = useState("");
  const [selectedAccountId, setSelectedAccountId] = useState("");
  const [selectedFileName, setSelectedFileName] = useState<string | null>(null);
  const [selectedFileBytes, setSelectedFileBytes] = useState<number[] | null>(null);
  const [selectedPreviewUrl, setSelectedPreviewUrl] = useState<string | null>(null);

  useEffect(() => {
    return () => {
      if (selectedPreviewUrl) {
        URL.revokeObjectURL(selectedPreviewUrl);
      }
    };
  }, [selectedPreviewUrl]);

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

  function clearSelectedFile() {
    setSelectedFileName(null);
    setSelectedFileBytes(null);
    setSelectedPreviewUrl((current) => {
      if (current) {
        URL.revokeObjectURL(current);
      }
      return null;
    });

    if (fileInputRef.current) {
      fileInputRef.current.value = "";
    }
  }

  async function handleFilePicked(event: ChangeEvent<HTMLInputElement>) {
    const file = event.target.files?.[0];
    if (!file) {
      return;
    }

    const bytes = Array.from(new Uint8Array(await file.arrayBuffer()));
    setSelectedFileName(file.name);
    setSelectedFileBytes(bytes);
    setSourcePath(file.name);
    setSelectedPreviewUrl((current) => {
      if (current) {
        URL.revokeObjectURL(current);
      }
      return URL.createObjectURL(file);
    });
  }

  const hasImportSource =
    Boolean(selectedFileBytes?.length) || Boolean(sourcePath.trim());

  const importMutation = useMutation({
    mutationFn: () =>
      importSkin({
        sourcePath: selectedFileBytes ? null : sourcePath.trim(),
        fileName: selectedFileName,
        sourceBytes: selectedFileBytes,
        displayName: displayName || null,
        modelVariant,
        tags: tags
          .split(",")
          .map((tag) => tag.trim())
          .filter(Boolean),
      }),
    onSuccess: async () => {
      clearSelectedFile();
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
            <span>Skin file</span>
            <input
              ref={fileInputRef}
              className="visually-hidden"
              type="file"
              accept=".png,image/png"
              onChange={handleFilePicked}
            />
            <div className="inline-input-row">
              <input
                value={sourcePath}
                onChange={(event) => {
                  clearSelectedFile();
                  setSourcePath(event.target.value);
                }}
                placeholder="Browse for a PNG or paste a local path"
              />
              <button type="button" onClick={() => fileInputRef.current?.click()}>
                Browse...
              </button>
            </div>
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

        {selectedPreviewUrl ? (
          <div className="skin-selection-preview">
            <img
              className="skin-preview-image"
              src={selectedPreviewUrl}
              alt={selectedFileName ?? "Selected skin preview"}
            />
            <div>
              <p className="eyebrow">Selected</p>
              <h4>{selectedFileName ?? "PNG selected"}</h4>
              <p className="muted">
                Blocksmith will copy this skin into the local library when you
                import it.
              </p>
            </div>
          </div>
        ) : null}

        <div className="panel-actions">
          <button
            className="primary"
            disabled={!hasImportSource}
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
              <article className="result-card skin-card" key={skin.id}>
                <div className="skin-card-preview-shell">
                  {skin.previewDataUrl ? (
                    <img
                      className="skin-preview-image"
                      src={skin.previewDataUrl}
                      alt={`${skin.displayName} preview`}
                      loading="lazy"
                    />
                  ) : (
                    <div className="skin-preview-fallback">No preview</div>
                  )}
                </div>

                <div className="skin-card-body">
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
                </div>
              </article>
            ))}
          </div>
        </section>
      ) : (
        <EmptyState
          eyebrow="Skins"
          title="No skins imported yet"
          body="Browse for a local PNG or paste a file path and Blocksmith will copy it into the launcher skin library."
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
