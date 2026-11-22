import { describe, expect, test } from 'bun:test';
import { mappingChoices, nextColour, notionDbUrl, type DbSchema } from '../../src/lib/notion';

const p = (name: string, kind: string) => ({ id: name, name, kind, options: [] });
const schema: DbSchema = {
  databaseId: 'db', dataSourceId: null, title: 'Tasks',
  properties: [p('Name', 'title'), p('Due', 'date'), p('Notes', 'rich_text'), p('Status', 'status'), p('Tags', 'multi_select'), p('Urgent', 'checkbox'), p('Owner', 'people'), p('Area', 'select')],
};

describe('notion helpers', () => {
  test('mappingChoices sorts the columns into roles', () => {
    const c = mappingChoices(schema);
    expect(c.title?.name).toBe('Name');
    expect(c.dates.map((x) => x.name)).toEqual(['Due']);
    expect(c.texts.map((x) => x.name)).toEqual(['Notes']);
    expect(c.fixed.map((x) => x.name)).toEqual(['Status', 'Tags', 'Urgent', 'Area']);
    expect(c.unused.map((x) => x.name)).toEqual(['Owner']);
  });
  test('notionDbUrl drops the dashes', () => expect(notionDbUrl('ab-cd')).toBe('https://www.notion.so/abcd'));
  test('nextColour takes the first unused', () => {
    expect(nextColour([])).toBe(1);
    expect(nextColour([{ colour: 1 }, { colour: 3 }])).toBe(2);
    expect(nextColour(Array.from({ length: 9 }, (_, i) => ({ colour: i + 1 })))).toBe(1);
  });
});
