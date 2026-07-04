export interface DateFilter {
  from?: string | null;
  to?: string | null;
}

export interface SqlFragment {
  sql: string;
  params: string[];
}

export function dateFilterFromSearch(searchParams: URLSearchParams): DateFilter {
  return {
    from: isoDate(searchParams.get("from")),
    to: isoDate(searchParams.get("to")),
  };
}

export function sessionDatePredicate(alias: string, filter?: DateFilter | null): SqlFragment {
  const clauses: string[] = [];
  const params: string[] = [];
  const from = isoDate(filter?.from);
  const to = isoDate(filter?.to);

  if (from != null) {
    clauses.push(`substr(${alias}.started_at, 1, 10) >= ?`);
    params.push(from);
  }
  if (to != null) {
    clauses.push(`substr(${alias}.started_at, 1, 10) <= ?`);
    params.push(to);
  }

  return { sql: clauses.join(" AND "), params };
}

export function whereClause(fragment: SqlFragment): string {
  return fragment.sql === "" ? "" : `WHERE ${fragment.sql}`;
}

function isoDate(value: string | null | undefined): string | null {
  if (value == null || !/^\d{4}-\d{2}-\d{2}$/.test(value)) {
    return null;
  }
  const date = new Date(`${value}T00:00:00.000Z`);
  return Number.isNaN(date.getTime()) || date.toISOString().slice(0, 10) !== value ? null : value;
}
