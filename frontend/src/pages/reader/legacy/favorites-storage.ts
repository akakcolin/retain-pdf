const STORAGE_PREFIX = "retainpdf.reader.favorites.";

function storageKey(jobId = "") {
  return `${STORAGE_PREFIX}${jobId || "unknown"}`;
}

function safeParse(text) {
  try {
    const value = JSON.parse(text || "[]");
    return Array.isArray(value) ? value : [];
  } catch {
    return [];
  }
}

export function createReaderFavoritesStore({
  jobId = "",
  storage = globalThis.localStorage,
} = {}) {
  const key = storageKey(jobId);

  function list() {
    return safeParse(storage?.getItem?.(key));
  }

  function save(items) {
    storage?.setItem?.(key, JSON.stringify(Array.isArray(items) ? items : []));
  }

  function add(item) {
    const next = [item, ...list()].slice(0, 200);
    save(next);
    return next;
  }

  return {
    add,
    list,
    save,
    storageKey: key,
  };
}
