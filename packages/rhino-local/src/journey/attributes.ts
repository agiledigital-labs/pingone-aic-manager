/** The one write path for shared `objectAttributes` that every built-in node merging into it uses. */
import { isDocument, setOwn } from "../idm/document.ts";
import type { Document } from "../idm/types.ts";

/**
 * Merges `added` into shared `objectAttributes`, keeping every key an earlier node wrote. A
 * non-object value there is missing behaviour, never replaced. Returns whether any key was there
 * before, so the caller can label the merge with its own evidence.
 */
export function mergeSharedAttributes(
  shared: Document,
  added: Document,
  type: string,
  missing: (name: string) => never,
): boolean {
  const previous = Object.hasOwn(shared, "objectAttributes") ? shared.objectAttributes : {};
  if (!isDocument(previous)) missing(`${type} over non-object shared objectAttributes`);
  setOwn(shared, "objectAttributes", { ...previous, ...added });
  return Object.keys(previous).length > 0;
}
