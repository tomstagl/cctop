// The view dispatcher: `model.view` picks which view draws the pane. Inline
// placement (the classic renderer's few rows above the prompt) always draws
// the Overview's short form, whatever view is selected.
import type { RenderElement } from 'claude-code';
import type { Model, Placement } from '../model';
import { renderAdvisor } from './advisor';
import { renderCoach, type CoachActions, type CoachElements } from './coach';
import { renderAgents } from './agents';
import { renderEvents } from './events';
import { renderFiles } from './files';
import { renderOverview, type OverviewActions, type ViewElements } from './overview';
import { renderTools } from './tools';

/** What the pane hands the views that press: the Button element and the closures over `$`. */
export type ViewActions = { el: CoachElements; coach: CoachActions; overview: OverviewActions };

export function renderView(
  model: Model,
  el: ViewElements,
  columns: number,
  placement: Placement,
  now: number,
  actions?: ViewActions,
): RenderElement {
  if (placement === 'inline') return renderOverview(model, el, columns, placement, now);
  switch (model.view) {
    case 'coach':
      return actions === undefined ? renderOverview(model, el, columns, placement, now) : renderCoach(model, actions.el, columns, actions.coach);
    case 'tools':
      return renderTools(model, el, columns, now);
    case 'agents':
      return renderAgents(model, el, columns, now);
    case 'files':
      return renderFiles(model, el, columns);
    case 'events':
      return renderEvents(model, el, columns);
    case 'advisor':
      return renderAdvisor(model, el, columns);
    case 'overview':
      return renderOverview(model, el, columns, placement, now, actions === undefined ? undefined : { el: actions.el, actions: actions.overview });
  }
}
