import type * as PageTree from 'fumadocs-core/page-tree';

/** Plain-object page tree, safe to pass from server to client components. */
export type TreeNode =
  | { type: 'separator'; name: string }
  | { type: 'page'; name: string; url: string }
  | { type: 'folder'; name: string; url?: string; children: TreeNode[] };

function nameToString(name: React.ReactNode): string {
  return typeof name === 'string' || typeof name === 'number' ? String(name) : '';
}

function convert(node: PageTree.Node): TreeNode | null {
  if (node.type === 'separator') {
    return { type: 'separator', name: nameToString(node.name) };
  }
  if (node.type === 'page') {
    return { type: 'page', name: nameToString(node.name), url: node.url };
  }
  if (node.type === 'folder') {
    let children = node.children
      .map(convert)
      .filter((child): child is TreeNode => child !== null);
    // The folder's index page (url is a prefix of every sibling's url) becomes
    // the folder row's own link instead of a duplicate child entry. `node.index`
    // isn't set when meta.json lists "index" explicitly, so detect it by URL.
    let url = node.index?.url;
    if (!url) {
      const index = children.find(
        (child) =>
          child.type === 'page' &&
          children.some(
            (other) =>
              other !== child &&
              other.type !== 'separator' &&
              other.url?.startsWith(`${child.url}/`),
          ),
      );
      if (index?.type === 'page') url = index.url;
    }
    children = children.filter(
      (child) => !(child.type === 'page' && child.url === url),
    );
    return { type: 'folder', name: nameToString(node.name), url, children };
  }
  return null;
}

export function serializeTree(tree: PageTree.Root): TreeNode[] {
  return tree.children
    .map(convert)
    .filter((child): child is TreeNode => child !== null);
}
