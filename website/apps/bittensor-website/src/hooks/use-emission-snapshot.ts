'use client';

import { useEffect, useState } from 'react';
import {
  DEFAULT_EMISSION_SNAPSHOT,
  type EmissionSnapshot,
  fetchEmissionSnapshot,
} from '@/lib/emission-snapshot';

export function useEmissionSnapshot() {
  const [snapshot, setSnapshot] = useState<EmissionSnapshot>(DEFAULT_EMISSION_SNAPSHOT);
  const [loading, setLoading] = useState(true);

  useEffect(() => {
    let cancelled = false;
    fetchEmissionSnapshot()
      .then((data) => {
        if (!cancelled) setSnapshot(data);
      })
      .finally(() => {
        if (!cancelled) setLoading(false);
      });
    return () => {
      cancelled = true;
    };
  }, []);

  return {snapshot, loading};
}
