import { useEffect, useState } from "react";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  bindProfileAccount,
  createLocalAccount,
  deleteAccount,
  listAccounts,
  signInMicrosoft,
} from "../../lib/tauri";
import type { ProfileSummary } from "../../types/api";
import { formatDateTime } from "../../lib/format";
import { EmptyState } from "../shared/EmptyState";

interface AccountsViewProps {
  launcherUnlocked: boolean;
  profiles: ProfileSummary[];
}

export function AccountsView({ launcherUnlocked, profiles }: AccountsViewProps) {
  const queryClient = useQueryClient();
  const [username, setUsername] = useState("");
  const [provider, setProvider] = useState("manual");
  const [selectedProfileId, setSelectedProfileId] = useState("");
  const [selectedAccountId, setSelectedAccountId] = useState("");

  const accountsQuery = useQuery({
    queryKey: ["accounts"],
    queryFn: listAccounts,
  });

  useEffect(() => {
    if (!selectedProfileId && profiles[0]) {
      setSelectedProfileId(profiles[0].id);
    }
  }, [profiles, selectedProfileId]);

  useEffect(() => {
    if (!selectedAccountId && accountsQuery.data?.[0]) {
      setSelectedAccountId(accountsQuery.data[0].id);
    }
  }, [accountsQuery.data, selectedAccountId]);

  const refresh = async () => {
    await Promise.all([
      queryClient.invalidateQueries({ queryKey: ["accounts"] }),
      queryClient.invalidateQueries({ queryKey: ["profiles"] }),
      queryClient.invalidateQueries({ queryKey: ["dashboard"] }),
    ]);
  };

  const createMutation = useMutation({
    mutationFn: () =>
      createLocalAccount({
        username,
        provider,
      }),
    onSuccess: async () => {
      setUsername("");
      await refresh();
    },
  });

  const microsoftMutation = useMutation({
    mutationFn: signInMicrosoft,
    onSuccess: refresh,
  });

  const deleteMutation = useMutation({
    mutationFn: (accountId: string) => deleteAccount(accountId),
    onSuccess: refresh,
  });

  const bindMutation = useMutation({
    mutationFn: () =>
      bindProfileAccount(selectedProfileId, selectedAccountId || null),
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: ["profiles"] });
    },
  });

  return (
    <div className="stack-grid">
      <section className="panel">
        <div className="panel-heading">
          <div>
            <p className="eyebrow">Accounts</p>
            <h3>Sign in and assign profiles</h3>
          </div>
        </div>

        <div className="toolbar-grid">
          <label>
            <span>Username</span>
            <input
              value={username}
              onChange={(event) => setUsername(event.target.value)}
              placeholder="PlayerOne"
            />
          </label>

          <label>
            <span>Provider</span>
            <select
              value={provider}
              onChange={(event) => setProvider(event.target.value)}
            >
              <option value="manual">manual</option>
              <option value="legacy">legacy</option>
            </select>
          </label>

          <label className="span-2">
            <span>Profile binding</span>
            <div className="inline-input-row">
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
              <select
                value={selectedAccountId}
                onChange={(event) => setSelectedAccountId(event.target.value)}
              >
                <option value="">No account</option>
                {(accountsQuery.data ?? []).map((account) => (
                  <option key={account.id} value={account.id}>
                    {account.username}
                  </option>
                ))}
              </select>
              <button onClick={() => bindMutation.mutate()}>Assign</button>
            </div>
          </label>
        </div>

        <div className="panel-actions">
          <button
            className="primary"
            disabled={!username.trim()}
            onClick={() => createMutation.mutate()}
          >
            {createMutation.isPending ? "Creating..." : "Create offline account"}
          </button>
          <button onClick={() => microsoftMutation.mutate()}>
            {microsoftMutation.isPending
              ? "Waiting for browser..."
              : "Sign in with Microsoft"}
          </button>
          <span className="muted">
            {launcherUnlocked
              ? "Downloads and launch are unlocked on this device. You can still use offline accounts for individual profiles."
              : "Sign in once with a Microsoft account that owns Minecraft to unlock downloads and launch. Offline accounts stay available after that."}
          </span>
        </div>
      </section>

      {(accountsQuery.data ?? []).length > 0 ? (
        <section className="panel">
          <div className="panel-heading">
            <div>
              <p className="eyebrow">Accounts</p>
              <h3>Stored sessions</h3>
            </div>
          </div>

          <div className="card-list">
            {(accountsQuery.data ?? []).map((account) => (
              <article className="result-card" key={account.id}>
                <div className="result-card-header">
                  <div>
                    <h4>{account.username}</h4>
                    <p className="muted">
                      {account.provider} / {account.uuid}
                    </p>
                  </div>
                  <button
                    className="danger-ghost"
                    onClick={() => deleteMutation.mutate(account.id)}
                  >
                    Remove
                  </button>
                </div>
                <p className="muted">
                  Session: {account.isAuthenticated ? "Online" : "Offline"}
                </p>
                <p className="muted">
                  Launch access:{" "}
                  {account.ownsMinecraft
                    ? "Unlocks downloads and launch"
                    : "Does not unlock downloads and launch"}
                </p>
                {account.ownershipVerifiedAt ? (
                  <p className="muted">
                    Verified owner: {formatDateTime(account.ownershipVerifiedAt)}
                  </p>
                ) : null}
                <p className="muted">
                  Active skin: {account.currentSkinId ?? "none"}
                </p>
              </article>
            ))}
          </div>
        </section>
      ) : (
        <EmptyState
          eyebrow="Accounts"
          title="No accounts stored yet"
          body="Create offline accounts for local play, then sign in once with Microsoft to unlock Minecraft downloads and launch."
        />
      )}

      {accountsQuery.error ? (
        <p className="error-text">{String(accountsQuery.error)}</p>
      ) : null}
      {createMutation.error ? (
        <p className="error-text">{String(createMutation.error)}</p>
      ) : null}
      {microsoftMutation.error ? (
        <p className="error-text">{String(microsoftMutation.error)}</p>
      ) : null}
      {deleteMutation.error ? (
        <p className="error-text">{String(deleteMutation.error)}</p>
      ) : null}
      {bindMutation.error ? (
        <p className="error-text">{String(bindMutation.error)}</p>
      ) : null}
    </div>
  );
}
