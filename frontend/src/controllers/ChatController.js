import * as aiService from '../services/aiService.js';
import { aiStore, addMessage, setProposal, setError } from '../stores/aiStore.js';
import { uiStore, pushToast } from '../stores/uiStore.js';
import { fileStore } from '../stores/fileStore.js';
import { compileStore } from '../stores/compileStore.js';
import { compileDocument } from './CompileController.js';

let currentSessionId = '';
aiStore.subscribe(s => currentSessionId = s.sessionId);

export async function requestAIPlan(request, model) {
  const { filePath } = getFileStoreValue();
  if (!filePath) {
    uiStore.update(ui => ({ ...ui, status: 'Please select a file first' }));
    return;
  }
  uiStore.update(ui => ({ ...ui, status: 'Planning with AI...' }));
  try {
    const plan = await aiService.requestAIPlan(filePath, request, model);
    uiStore.update(ui => ({ ...ui, status: 'AI plan ready', activeRightTab: 'ai' }));
    return plan;
  } catch (e) {
    uiStore.update(ui => ({ ...ui, status: `AI error: ${e.message}` }));
    return null;
  }
}

export async function handleChatMessage(content, timestamp) {
  const { filePath } = getFileStoreValue();
  addMessage('user', content);
  
  uiStore.update(ui => ({ ...ui, status: 'AI is thinking...' }));
  
  let currentResponse = '';
  aiStore.update(a => ({ ...a, status: 'streaming', streamingMessage: '' }));
  
  try {
    await aiService.agentLoopStream(
      filePath || 'main.tex',
      content,
      (data) => {
        aiStore.update(a => {
          if (data.type === 'progress') {
            currentResponse += `[Progress] ${data.message}\n`;
            return { ...a, streamingMessage: currentResponse };
          } else if (data.type === 'error') {
            currentResponse += `[Error] ${data.message}\n`;
            return { ...a, streamingMessage: currentResponse };
          } else if (data.type === 'proposal') {
            currentResponse += `[Done] Proposal ready for review.\n`;
            try {
              const payload = typeof data.message === 'string' ? JSON.parse(data.message) : data.message;
              setTimeout(() => {
                uiStore.update(ui => ({ ...ui, activeRightTab: 'ai' }));
                setProposal(payload);
              }, 0);
            } catch (e) {}
            return { ...a, streamingMessage: currentResponse };
          }
          return a;
        });
      },
      (error) => {
        console.error(error);
        pushToast('AI stream error', 'error');
      }
    );
    
    addMessage('assistant', currentResponse || 'No response from agent.');
  } catch (err) {
    console.error(err);
    pushToast('Failed to connect to agent', 'error');
    addMessage('assistant', 'Sorry, I encountered an error.');
  } finally {
    aiStore.update(a => ({ ...a, status: 'ready', streamingMessage: '' }));
    uiStore.update(ui => ({ ...ui, status: 'Ready' }));
  }
}

export async function handleApplyHunks(hunks, selected, aiPlan) {
  const { filePath } = getFileStoreValue();
  if (!hunks || !selected) {
    pushToast('No hunks to apply', 'error');
    return;
  }

  let edits = [];
  let selectedHunkIds = [];
  
  if (aiPlan && aiPlan.edits) {
    edits = aiPlan.edits;
    selected.forEach((isSelected, idx) => {
      if (isSelected && hunks[idx] && hunks[idx].id) {
        selectedHunkIds.push(hunks[idx].id);
      }
    });
  }

  uiStore.update(ui => ({ ...ui, status: 'Applying selected edits...' }));
  try {
    const res = await aiService.applySelectedHunks(filePath, edits, selectedHunkIds);
    if (res && res.success) {
      uiStore.update(ui => ({ ...ui, status: 'Selected edits applied', activeRightTab: 'pdf' }));
      pushToast('Selected changes applied', 'success');
      
      const fileRes = await fileService.loadFile(filePath); // Note: Need to import fileService if needed here, but wait, loadFile from FileController is better.
      fileStore.update(f => ({ ...f, content: fileRes.content }));
      
      compileDocument(filePath);
      return true;
    } else {
      throw new Error(res.error || 'Unknown error');
    }
  } catch (err) {
    uiStore.update(ui => ({ ...ui, status: `Apply failed: ${err.message}` }));
    pushToast(`Apply failed: ${err.message}`, 'error');
    return false;
  }
}

function getFileStoreValue() {
  let value;
  const unsubscribe = fileStore.subscribe(v => value = v);
  unsubscribe();
  return value;
}
