import {
  createColumnHelper,
  flexRender,
  getCoreRowModel,
  useReactTable,
} from "@tanstack/react-table";
import type { ProfileSummary } from "../../types/api";
import { formatDateTime } from "../../lib/format";

interface ProfileTableProps {
  profiles: ProfileSummary[];
  onDuplicate: (profile: ProfileSummary) => void;
  onDelete: (profile: ProfileSummary) => void;
}

const columnHelper = createColumnHelper<ProfileSummary>();

const columns = [
  columnHelper.accessor("name", {
    header: "Profile",
    cell: (ctx) => (
      <div className="table-primary">
        <strong>{ctx.getValue()}</strong>
        <span>{ctx.row.original.minecraftVersion}</span>
      </div>
    ),
  }),
  columnHelper.accessor("profileType", {
    header: "Type",
    cell: (ctx) => (
      <span className={`chip ${ctx.getValue()}`}>{ctx.getValue()}</span>
    ),
  }),
  columnHelper.accessor("loaderVersion", {
    header: "Loader",
    cell: (ctx) => ctx.getValue() ?? "None",
  }),
  columnHelper.accessor("directoryPath", {
    header: "Directory",
    cell: (ctx) => <span className="mono">{ctx.getValue()}</span>,
  }),
  columnHelper.accessor("lastPlayedAt", {
    header: "Last Played",
    cell: (ctx) => formatDateTime(ctx.getValue()),
  }),
  columnHelper.display({
    id: "actions",
    header: "Actions",
    cell: (ctx) => (
      <div className="row-actions">
        <button
          onClick={() =>
            (
              ctx.table.options.meta as
                | {
                    duplicate?: (profile: ProfileSummary) => void;
                    remove?: (profile: ProfileSummary) => void;
                  }
                | undefined
            )?.duplicate?.(ctx.row.original)
          }
        >
          Duplicate
        </button>
        <button
          className="danger-ghost"
          onClick={() =>
            (
              ctx.table.options.meta as
                | {
                    duplicate?: (profile: ProfileSummary) => void;
                    remove?: (profile: ProfileSummary) => void;
                  }
                | undefined
            )?.remove?.(ctx.row.original)
          }
        >
          Delete
        </button>
      </div>
    ),
  }),
];

export function ProfileTable({
  profiles,
  onDuplicate,
  onDelete,
}: ProfileTableProps) {
  const table = useReactTable({
    data: profiles,
    columns,
    getCoreRowModel: getCoreRowModel(),
    meta: {
      duplicate: onDuplicate,
      remove: onDelete,
    },
  });

  return (
    <div className="table-shell">
      <table>
        <thead>
          {table.getHeaderGroups().map((headerGroup) => (
            <tr key={headerGroup.id}>
              {headerGroup.headers.map((header) => (
                <th key={header.id}>
                  {header.isPlaceholder
                    ? null
                    : flexRender(
                        header.column.columnDef.header,
                        header.getContext(),
                      )}
                </th>
              ))}
            </tr>
          ))}
        </thead>
        <tbody>
          {table.getRowModel().rows.map((row) => (
            <tr key={row.id}>
              {row.getVisibleCells().map((cell) => (
                <td key={cell.id}>
                  {flexRender(cell.column.columnDef.cell, cell.getContext())}
                </td>
              ))}
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}
