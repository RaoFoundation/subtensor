'use client';

import {useState, useEffect, FC, Dispatch, SetStateAction, ComponentProps} from 'react';

import ReactMarkdown from 'react-markdown';
import remarkGfm from 'remark-gfm';
import remarkMath from 'remark-math';
import rehypeRaw from 'rehype-raw';
import rehypeKatex from 'rehype-katex';

// The fumadocs packages pull newer `unified` types into the tree, which no
// longer structurally match react-markdown v8's bundled plugin types.
const rehypePlugins = [rehypeRaw, rehypeKatex] as ComponentProps<
  typeof ReactMarkdown
>['rehypePlugins'];

import {Code} from '../Code/Code';
import {NormalComponents} from 'react-markdown/lib/complex-types';
import {SpecialComponents} from 'react-markdown/lib/ast-to-react';
import {
  H1,
  H2,
  H3,
  H4,
  Li,
  BaseLink,
  Ol,
  Text,
  Ul,
  createImage,
  Bold,
  Table,
  Accordion,
  CodeEl,
  slugify,
} from './components';
import {SectionInViewLink} from '../SectionInView/SectionInView';

const replaceLatexDelimiters = (markdown: string) => {
  return markdown.replace(/\\+\[/g, '$$').replace(/\\+\]/g, '$$');
};

export type MarkdownComponentsType = Partial<
  Omit<NormalComponents, keyof SpecialComponents> & SpecialComponents
>;

export type SectionLink = {
  label: string;
  href: string;
};

export type Props = {
  assetsPath?: string;
  children: string;
  components?: MarkdownComponentsType;
  setSectionLinks?: Dispatch<SetStateAction<SectionLink[]>>;
};

export const Markdown: FC<Props> = ({children, assetsPath = '', components, setSectionLinks}) => {
  const [isClient, setIsClient] = useState(false);
  const [processedMarkdown, setProcessedMarkdown] = useState('');

  const defaultComponents: MarkdownComponentsType = {
    a: BaseLink,
    h1: H1,
    h2: H2,
    h3: H3,
    h4: H4,
    li: Li,
    ul: Ul,
    ol: Ol,
    p: Text,
    // @ts-ignore
    accordion: Accordion,
    table: Table,
    strong: Bold,
    img: createImage(assetsPath),
    code: CodeEl,
    pre: ({node, ...props}) => {
      // @ts-ignore
      const childProps = props?.children?.[0]?.props;
      const className = childProps?.className;
      const language = className ? className.split('-')[1] : '';

      const code = childProps.children.join('');

      return <Code language={language} code={code} />;
    },
  };

  useEffect(() => {
    setIsClient(true);
    setProcessedMarkdown(replaceLatexDelimiters(children));
  }, [children]);

  useEffect(() => {
    if (!setSectionLinks) {
      return;
    }

    const links: SectionInViewLink[] = [];
    processedMarkdown.match(/\n(#{3}\s.+(?=\n))/g)?.forEach((match) => {
      const label = match.replace(/\n#{3} /g, '').replaceAll('*', '');
      const href = `#${slugify(label)}`;
      links.push({label, href});
    });

    setSectionLinks(links);
  }, [processedMarkdown]);

  if (!isClient) {
    return null;
  }

  return (
    <ReactMarkdown
      remarkPlugins={[remarkGfm, remarkMath]}
      rehypePlugins={rehypePlugins}
      components={{...defaultComponents, ...components}}
    >
      {processedMarkdown}
    </ReactMarkdown>
  );
};
