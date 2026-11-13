// Ordinary module initialization: deliberately no Vitest imports or hooks.
(globalThis as typeof globalThis & { precollectHelper?: string }).precollectHelper = "initialized";
