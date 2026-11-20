import {useLayoutEffect, type RefObject} from 'react';

const REVEAL = '.example, .feature, .prose > section, .prose > .card-list > .card, .post-list > li';

export function prefersReducedMotion(): boolean {
  return typeof window !== 'undefined' && window.matchMedia('(prefers-reduced-motion: reduce)').matches;
}

/**
 * Enables motion on the root (class "motion") unless the user prefers reduced motion, then reveals blocks once as
 * they scroll into view. Content stays visible without the "motion" class (no JS, or reduced motion).
 */
export function useMotion(root: RefObject<HTMLElement | null>, routeKey: string) {
  useLayoutEffect(() => {
    const el = root.current;
    if (!el) return;
    if (prefersReducedMotion() || typeof IntersectionObserver === 'undefined') {
      el.classList.remove('motion');
      return;
    }
    el.classList.add('motion');
    const io = new IntersectionObserver(
      (entries) => {
        for (const e of entries) {
          if (!e.isIntersecting) continue;
          e.target.classList.add('is-visible');
          io.unobserve(e.target);
        }
      },
      {threshold: 0.1, rootMargin: '0px 0px -6% 0px'},
    );
    el.querySelectorAll<HTMLElement>(REVEAL).forEach((t) => {
      const idx = Array.prototype.indexOf.call(t.parentElement?.children ?? [], t);
      t.style.setProperty('--i', String(Math.min(Math.max(idx, 0), 4)));
      t.classList.add('reveal');
      io.observe(t);
    });
    el.querySelectorAll<HTMLElement>('.bars').forEach((t) => io.observe(t));
    return () => io.disconnect();
  }, [root, routeKey]);
}
