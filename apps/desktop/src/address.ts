// Comparing camera addresses as the settings store them.

/** Phase 1 stored the bare address, later versions add the PTP/IP port. */
export const sameAddress = (a: string, b: string) =>
  a.trim().replace(/:15740$/, '') === b.trim().replace(/:15740$/, '');
