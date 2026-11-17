import { useState, useEffect, useCallback } from 'react';
import { toast } from 'sonner';
import { invoke as invokeTauri } from '@tauri-apps/api/core';

export function useTemplates() {
  const [availableTemplates, setAvailableTemplates] = useState<Array<{
    id: string;
    name: string;
    emoji?: string | null;
  }>>([]);
  // null means "let Conclave choose" (stored choice, title keyword, model auto-pick, or Standard).
  const [selectedTemplate, setSelectedTemplate] = useState<string | null>(null);

  // Fetch available templates on mount
  useEffect(() => {
    const fetchTemplates = async () => {
      try {
        const res = await invokeTauri<{ templates: Array<{ id: string; name: string; emoji?: string | null }>; errors: string[] }>('api_list_templates');
        console.log('Available templates:', res.templates);
        setAvailableTemplates(res.templates.map(({ id, name, emoji }) => ({ id, name, emoji })));
      } catch (error) {
        console.error('Failed to fetch templates:', error);
        toast.error('Could not load templates', { description: error instanceof Error ? error.message : String(error) });
      }
    };
    fetchTemplates();
  }, []);

  // Regeneration (which the caller triggers) shows its own feedback
  const handleTemplateSelection = useCallback((templateId: string, _templateName: string) => {
    setSelectedTemplate(templateId);
  }, []);

  return {
    availableTemplates,
    selectedTemplate,
    handleTemplateSelection,
  };
}
