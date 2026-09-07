"""Inspect captured DOM structure locally; no requests and no query strings in the report."""
from html.parser import HTMLParser
from pathlib import Path
import json

ROOT = Path(__file__).resolve().parent
VOID = set('area base br col embed hr img input link meta param source track wbr'.split())


class Node:
    def __init__(self, tag, attrs=(), parent=None):
        self.tag, self.attrs, self.parent = tag, dict(attrs), parent
        self.children = []

    def text(self):
        if self.tag in ['script', 'style', 'noscript', 'template']:
            return ''
        return ' '.join(x if isinstance(x, str) else x.text() for x in self.children)

    def descendants(self):
        yield self
        for child in self.children:
            if isinstance(child, Node):
                yield from child.descendants()

    def describe(self):
        return dict(tag=self.tag, attrs={k: self.attrs[k] for k in ['class', 'role'] if k in self.attrs},
            text_characters=len(' '.join(self.text().split())),
            links=sum(n.tag == 'a' and bool(n.attrs.get('href')) for n in self.descendants()),
            direct_children=[n.tag for n in self.children if isinstance(n, Node)])


class Parser(HTMLParser):
    def __init__(self):
        super().__init__(convert_charrefs=True)
        self.root = Node('document')
        self.stack = [self.root]

    def handle_starttag(self, tag, attrs):
        node = Node(tag, attrs, self.stack[-1])
        self.stack[-1].children.append(node)
        if tag not in VOID:
            self.stack.append(node)

    def handle_endtag(self, tag):
        for i in range(len(self.stack)-1, 0, -1):
            if self.stack[i].tag == tag:
                del self.stack[i:]
                break

    def handle_data(self, text):
        self.stack[-1].children.append(text)


def inspect(name):
    parser = Parser()
    parser.feed((ROOT / 'state/public-html' / (name + '.html')).read_text(encoding='utf-8'))
    rows = []
    for node in parser.root.descendants():
        if node.tag not in ['h1', 'h2', 'h3']:
            continue
        ancestors = []
        parent = node.parent
        for _ in range(5):
            if parent is None:
                break
            ancestors.append(parent.describe())
            parent = parent.parent
        rows.append(dict(heading=' '.join(node.text().split())[:180], node=node.describe(), ancestors=ancestors))
    (ROOT / 'state' / (name + '-markup.json')).write_text(json.dumps(rows, indent=2) + '\n', encoding='utf-8')
    return rows


if __name__ == '__main__':
    for name in ['github-explore', 'indeed-jobs', 'google-search']:
        rows = inspect(name)
        print(name, len(rows), 'headings; DOM report saved privately')
