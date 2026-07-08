const makeRoute = (path, { parent = HOME, title, hash } = {}) => ({
  path,
  parent,
  titleI18nKey: title,
  linkTo: (params) => ({
    pathname: path,
    query: params,
    hash: hash,
  }),
})

// IMPORTANT The following routes MUST be in sync with those files on `pages/`
// which export a React component that Next.js will serve as an app route.

export const HOME = makeRoute('/', {
  parent: null,
  title: 'title:site',
})

export const ABOUT = makeRoute('/about', {
  title: 'title:about',
})

export const BLOG = makeRoute('/blog', {
  title: 'title:blog',
})

export const POST_DETAIL = makeRoute('/blog/[slug]', {
  title: null,
})

export const LEGAL = makeRoute('/legal', {
  title: 'title:legal',
})

export const TECHNOLOGY = makeRoute('/technology', {
  title: 'title:technology',
})

export const TECHNOLOGY_DETAIL = makeRoute('/technology/[...slug]', {
  title: 'title:technology',
})

export const COOKIES_POLICY = makeRoute('/legal', {
  title: 'title:cookies-policy',
  hash: 'cookies-policy',
})

export const ERROR_500 = makeRoute('/500', {
  title: 'title:site',
})

export const WALLET = makeRoute('/wallet', {
  title: 'title:site',
})
