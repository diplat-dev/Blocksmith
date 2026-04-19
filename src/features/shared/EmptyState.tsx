interface EmptyStateProps {
  eyebrow: string;
  title: string;
  body: string;
}

export function EmptyState({ eyebrow, title, body }: EmptyStateProps) {
  return (
    <section className="panel empty-state">
      <p className="eyebrow">{eyebrow}</p>
      <h3>{title}</h3>
      <p>{body}</p>
    </section>
  );
}

