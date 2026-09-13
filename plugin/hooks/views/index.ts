// The view dispatcher: `model.view` picks which view draws the pane. Inline
// placement (the classic renderer's few rows above the prompt) always draws
// the Overview's short form, whatever view is selected.
import type { RenderElement } from 'claude-code';
import type { Model, Placement } from '../model';
import { renderAdvisor } from './advisor';
import { renderAgents } from './agents';
import { renderEvents } from './events';
import { renderFiles } from './files';
import { renderOverview, type ViewElements } from './overview';
import { renderTools } from './tools';

export function renderView(model: Model, el: ViewElements, columns: number, placement: Placement, now: number): RenderElement {
  if (placement === 'inline') return renderOverview(model, el, columns, placement, now);
  switch (model.view) {
    case 'tools':
      return renderTools(model, el, now);
    case 'agents':
      return renderAgents(model, el, now);
    case 'files':
      return renderFiles(model, el);
    case 'events':
      return renderEvents(model, el);
    case 'advisor':
      return renderAdvisor(model, el, columns);
    case 'overview':
      return renderOverview(model, el, columns, placement, now);
  }
}
