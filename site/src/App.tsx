import {Route, Routes} from 'react-router';
import {Layout} from './components/Layout';
import {Blog, BlogPost} from './pages/Blog';
import {Docs} from './pages/Docs';
import {Examples} from './pages/Examples';
import {Home} from './pages/Home';
import {Learn} from './pages/Learn';
import {NotFound} from './pages/NotFound';
import {Playground} from './pages/Playground';

export function App() {
  return (
    <Routes>
      <Route element={<Layout />}>
        <Route index element={<Home />} />
        <Route path="learn" element={<Learn />} />
        <Route path="docs" element={<Docs />} />
        <Route path="examples" element={<Examples />} />
        <Route path="blog" element={<Blog />} />
        <Route path="blog/:slug" element={<BlogPost />} />
        <Route path="playground" element={<Playground />} />
        <Route path="*" element={<NotFound />} />
      </Route>
    </Routes>
  );
}
