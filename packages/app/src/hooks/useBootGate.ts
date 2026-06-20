import { useEffect, useMemo, useState } from 'react';

export interface BootStep {
  id: string;
  label: string;
  status: 'pending' | 'active' | 'done';
}

const MIN_BOOT_MS = 2200;
const EXIT_MS = 520;

function prefersReducedMotion(): boolean {
  return typeof window !== 'undefined' && window.matchMedia('(prefers-reduced-motion: reduce)').matches;
}

export function useBootGate(input: {
  projectReady: boolean;
  bridgeReady: boolean;
  registryReady: boolean;
}) {
  const { projectReady, bridgeReady, registryReady } = input;
  const reducedMotion = prefersReducedMotion();
  const minBootMs = reducedMotion ? 400 : MIN_BOOT_MS;
  const exitMs = reducedMotion ? 120 : EXIT_MS;

  const [minElapsed, setMinElapsed] = useState(false);
  const [exiting, setExiting] = useState(false);
  const [dismissed, setDismissed] = useState(false);

  useEffect(() => {
    const timer = window.setTimeout(() => setMinElapsed(true), minBootMs);
    return () => window.clearTimeout(timer);
  }, [minBootMs]);

  const systemsReady = projectReady && bridgeReady && registryReady;

  const steps = useMemo<BootStep[]>(() => {
    const prefsDone = projectReady;
    const bridgeDone = bridgeReady;
    const panesDone = registryReady;
    const orchestratorDone = systemsReady && minElapsed;

    const next = (done: boolean, active: boolean): BootStep['status'] => {
      if (done) return 'done';
      if (active) return 'active';
      return 'pending';
    };

    return [
      {
        id: 'prefs',
        label: 'Loading workspace preferences',
        status: next(prefsDone, !prefsDone),
      },
      {
        id: 'bridge',
        label: 'Connecting local bridge',
        status: next(bridgeDone, prefsDone && !bridgeDone),
      },
      {
        id: 'panes',
        label: 'Restoring terminal panes',
        status: next(panesDone, bridgeDone && !panesDone),
      },
      {
        id: 'orchestrator',
        label: 'Preparing orchestrator',
        status: next(orchestratorDone, panesDone && !orchestratorDone),
      },
    ];
  }, [projectReady, bridgeReady, registryReady, systemsReady, minElapsed]);

  const progress = useMemo(() => {
    const completed = steps.filter((step) => step.status === 'done').length;
    const activeBonus = steps.some((step) => step.status === 'active') ? 0.12 : 0;
    return Math.min(1, completed / steps.length + activeBonus);
  }, [steps]);

  useEffect(() => {
    if (!systemsReady || !minElapsed || dismissed) return;
    setExiting(true);
    const timer = window.setTimeout(() => setDismissed(true), exitMs);
    return () => window.clearTimeout(timer);
  }, [systemsReady, minElapsed, dismissed, exitMs]);

  return {
    showBoot: !dismissed,
    exiting,
    steps,
    progress,
  };
}
