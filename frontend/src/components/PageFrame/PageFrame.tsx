import React from 'react';
import { cn } from '@/lib/utils';

export type PageFrameProps = {
  variant: 'list' | 'reading' | 'dashboard' | 'form';
  title?: React.ReactNode;
  breadcrumb?: React.ReactNode;
  actions?: React.ReactNode;
  rail?: React.ReactNode;
  subnav?: React.ReactNode;
  children: React.ReactNode;
};

// Left-aligned frame (the shell supplies the left padding); list and reading are height-bounded, dashboard and form scroll as a page.
export function PageFrame({ variant, title, breadcrumb, actions, rail, subnav, children }: PageFrameProps): JSX.Element {
  const bounded = variant === 'list' || variant === 'reading';

  const header = (
    <div className="shrink-0">
      {breadcrumb && <div className="mb-2">{breadcrumb}</div>}
      {(title || actions) && (
        <div className="mb-6 flex flex-wrap items-center justify-between gap-3">
          {title && <h1 className="text-xl font-semibold">{title}</h1>}
          {actions && <div className="flex items-center gap-2">{actions}</div>}
        </div>
      )}
    </div>
  );

  let body: React.ReactNode;
  if (variant === 'list') {
    body = <div className={cn('flex min-h-0 flex-1 flex-col', subnav && 'min-w-0')}>{children}</div>;
  } else if (variant === 'form') {
    body = <div className="w-full min-w-0 max-w-[880px]">{children}</div>;
  } else if (variant === 'reading') {
    body = (
      <div
        className={cn(
          'grid min-h-0 min-w-0 flex-1 gap-10',
          '@[1280px]/frame:grid-cols-[minmax(0,80ch)_minmax(360px,560px)]',
          // Stacked (narrow): the text column keeps a bounded height with its own scroll (transcript virtualizer, ?line= jump), the page scrolls to the rail; side by side each column scrolls itself.
          rail && 'content-start overflow-y-auto @[1280px]/frame:grid-rows-1 @[1280px]/frame:overflow-visible',
        )}
      >
        <div className={cn('flex min-w-0 max-w-[80ch] flex-col', rail ? 'h-[75vh] overflow-y-auto @[1280px]/frame:h-auto @[1280px]/frame:min-h-0' : 'min-h-0 overflow-y-auto')}>{children}</div>
        {rail && <div className="min-w-0 @[1280px]/frame:min-h-0 @[1280px]/frame:overflow-y-auto">{rail}</div>}
      </div>
    );
  } else {
    body = children;
  }

  if (subnav && variant !== 'dashboard') {
    const row = {
      form: 'flex gap-8',
      list: 'flex min-h-0 flex-1 gap-8',
      reading: 'flex min-h-0 flex-1 flex-col gap-8 @[1280px]/frame:flex-row',
    }[variant];
    const nav = {
      form: 'w-[220px] shrink-0',
      list: 'w-[220px] shrink-0 overflow-y-auto',
      reading: 'max-h-48 shrink-0 overflow-y-auto @[1280px]/frame:max-h-none @[1280px]/frame:w-[300px]',
    }[variant];
    body = (
      <div className={row}>
        <div className={nav}>{subnav}</div>
        {variant === 'form' ? <div className="min-w-0 flex-1">{body}</div> : body}
      </div>
    );
  }

  if (bounded) {
    return (
      <div className="@container/frame flex h-full min-h-0 min-w-0 flex-col py-8 pr-8">
        {header}
        {body}
      </div>
    );
  }
  return (
    <div className="h-full overflow-y-auto">
      <div className={cn('@container/frame min-w-0 py-8 pr-8', variant === 'dashboard' && 'max-w-[1600px]')}>
        {header}
        {body}
      </div>
    </div>
  );
}

export default PageFrame;
