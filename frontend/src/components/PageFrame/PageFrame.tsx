import React from 'react';
import { cn } from '@/lib/utils';
import { SplitHandle, useSplitRatio } from '@/hooks/useSplitRatio';

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
  // One split shared by every reading page with a rail, so the meeting page and Digests remember the same width.
  const split = useSplitRatio({ storageKey: 'conclave.reading.mainRatio', defaultRatio: 0.6, min: 0.35, max: 0.8, label: 'summary and side panel', gutter: 32 });
  const nav = useSplitRatio({ storageKey: 'conclave.subnav.width', defaultRatio: variant === 'reading' ? 300 : 220, min: 160, max: 520, label: 'navigation', px: 'start', offset: 16 });
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
        ref={rail ? split.containerRef : undefined}
        {...(rail ? split.containerHandlers : {})}
        // The rail never gets narrower than 320px; the fr split fills the frame side by side.
        style={rail ? ({ '--reading-cols': `minmax(0,${split.ratio}fr) auto minmax(320px,${1 - split.ratio}fr)` } as React.CSSProperties) : undefined}
        className={cn(
          'grid min-h-0 min-w-0 flex-1 gap-10',
          rail ? '@[1280px]/frame:grid-cols-[var(--reading-cols)] @[1280px]/frame:gap-x-3' : '@[1280px]/frame:grid-cols-[minmax(0,80ch)_minmax(360px,560px)]',
          // Stacked (narrow): the text column keeps a bounded height with its own scroll (transcript virtualizer, ?line= jump), the page scrolls to the rail; side by side each column scrolls itself.
          rail && 'content-start overflow-y-auto @[1280px]/frame:grid-rows-1 @[1280px]/frame:overflow-visible',
        )}
      >
        <div className={cn('flex min-w-0 flex-col', rail ? 'h-[75vh] overflow-y-auto @[1280px]/frame:h-auto @[1280px]/frame:min-h-0' : 'min-h-0 max-w-[80ch] overflow-y-auto')}>{children}</div>
        {rail && <SplitHandle {...split.separatorProps} className="hidden @[1280px]/frame:flex" />}
        {rail && <div className="min-w-0 @[1280px]/frame:min-h-0 @[1280px]/frame:overflow-y-auto">{rail}</div>}
      </div>
    );
  } else {
    body = children;
  }

  if (subnav && variant !== 'dashboard') {
    const row = {
      form: 'flex gap-3',
      list: 'flex min-h-0 flex-1 gap-3',
      reading: 'flex min-h-0 flex-1 flex-col gap-8 @[1280px]/frame:flex-row @[1280px]/frame:gap-3',
    }[variant];
    const navCls = {
      form: 'shrink-0',
      list: 'shrink-0 overflow-y-auto',
      reading: 'max-h-48 shrink-0 overflow-y-auto @[1280px]/frame:max-h-none @[1280px]/frame:w-[var(--subnav-w)]',
    }[variant];
    // Reading stacks the nav above the body when narrow, so its handle only shows side by side.
    const stacked = variant === 'reading';
    body = (
      <div ref={nav.containerRef} {...nav.containerHandlers} className={row} style={{ '--subnav-w': `${nav.ratio}px` } as React.CSSProperties}>
        <div className={navCls} style={stacked ? undefined : { width: nav.ratio }}>{subnav}</div>
        <SplitHandle {...nav.separatorProps} className={stacked ? 'hidden @[1280px]/frame:flex' : 'hidden md:flex'} />
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
