import type { JsonObject, JsonValue, StandardSchema } from "./types.ts";
export declare function isPlainObject(value: unknown): value is Record<string, unknown>;
export declare function isStandardSchema(value: unknown): value is StandardSchema;
export declare function formatValue(value: unknown): string;
export declare function suggestKey(unknown: string, allowed: readonly string[]): string | undefined;
export declare function unknownKeyError(path: string, key: string, allowed: readonly string[]): Error;
export declare function parseJsonValue(raw: unknown, path: string): JsonValue;
export declare function assertDenseArray(value: readonly unknown[], path: string): void;
export declare function parseJsonObject(raw: unknown, path: string): JsonObject;
