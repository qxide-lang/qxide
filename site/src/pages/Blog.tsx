import {Link, useParams} from 'react-router';
import {usePageMeta} from '../lib/meta';
import {NotFound} from './NotFound';
import {POSTS, formatDate} from './posts';

export function Blog() {
  usePageMeta('Qxide blog', 'Release announcements and design notes from the Qxide project.');

  return (
    <section className="prose" aria-labelledby="blog-title">
      <header className="page-head">
        <h1 id="blog-title">Blog</h1>
        <p className="lede">Release announcements and notes on where Qxide is going.</p>
      </header>
      <ul className="post-list">
        {POSTS.map((p) => (
          <li key={p.slug}>
            <article>
              <h2>
                <Link to={`/blog/${p.slug}`}>{p.title}</Link>
              </h2>
              <p className="post-date">
                <time dateTime={p.date}>{formatDate(p.date)}</time>
              </p>
              <p>{p.summary}</p>
            </article>
          </li>
        ))}
      </ul>
    </section>
  );
}

export function BlogPost() {
  const {slug} = useParams();
  const post = POSTS.find((p) => p.slug === slug);
  usePageMeta(
    post ? `${post.title} | Qxide blog` : 'Post not found | Qxide',
    post ? post.summary : 'This blog post does not exist.',
  );
  if (!post) return <NotFound />;

  return (
    <article className="prose">
      <p className="crumb">
        <Link to="/blog">← All posts</Link>
      </p>
      <header className="page-head">
        <h1>{post.title}</h1>
        <p className="post-date">
          <time dateTime={post.date}>{formatDate(post.date)}</time>
        </p>
      </header>
      {post.body}
    </article>
  );
}
