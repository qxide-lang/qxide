import {useEffect} from 'react';

export const GITHUB = 'https://github.com/qxide-lang/qxide';
export const GITHUB_BLOB = `${GITHUB}/blob/main`;
export const GITHUB_TREE = `${GITHUB}/tree/main`;

function setMeta(selector: string, create: () => HTMLMetaElement, content: string) {
  const el = document.head.querySelector<HTMLMetaElement>(selector) ?? create();
  el.setAttribute('content', content);
  if (!el.isConnected) document.head.appendChild(el);
}

/** Sets the document title and description for the current route (static hosting has no server-side meta). */
export function usePageMeta(title: string, description: string) {
  useEffect(() => {
    document.title = title;
    setMeta(
      'meta[name="description"]',
      () => Object.assign(document.createElement('meta'), {name: 'description'}),
      description,
    );
    setMeta(
      'meta[property="og:title"]',
      () => {
        const m = document.createElement('meta');
        m.setAttribute('property', 'og:title');
        return m;
      },
      title,
    );
    setMeta(
      'meta[property="og:description"]',
      () => {
        const m = document.createElement('meta');
        m.setAttribute('property', 'og:description');
        return m;
      },
      description,
    );
  }, [title, description]);
}
