// Cloudflare Pages Functions middleware: send the apex domain to www with a 301.
// Only the exact host qxide.dev is touched, so qxide.pages.dev and preview URLs keep working.
// The installer paths are skipped so that public/_redirects (302 to the latest GitHub release) answers directly.

const APEX_HOST = 'qxide.dev';
const CANONICAL_HOST = 'www.qxide.dev';
const INSTALLER_PATHS = new Set(['/install.sh', '/install.ps1']);

type Context = {request: Request; next: () => Promise<Response>};

export const onRequest = async (context: Context): Promise<Response> => {
  const url = new URL(context.request.url);
  if (url.hostname.toLowerCase() === APEX_HOST && !INSTALLER_PATHS.has(url.pathname)) {
    url.protocol = 'https:';
    url.host = CANONICAL_HOST; // assigning host also drops any explicit port
    return Response.redirect(url.toString(), 301);
  }
  return context.next();
};
