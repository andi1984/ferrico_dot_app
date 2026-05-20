#![cfg_attr(
  all(not(debug_assertions), target_os = "windows"),
  windows_subsystem = "windows"
)]

use std::fs;
use std::path::PathBuf;
use tauri::State;
use serde::{Deserialize, Serialize};
use std::sync::Mutex;

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct Todo {
  pub id: u64,
  pub text: String,
  pub done: bool,
}

pub struct AppState {
  todos: Mutex<Vec<Todo>>,
  data_dir: PathBuf,
}

#[tauri::command]
fn get_todos(state: State<'_, AppState>) -> Vec<Todo> {
  state.todos.lock().unwrap().clone()
}

#[tauri::command]
fn add_todo(text: String, state: State<'_, AppState>) -> Todo {
  let mut todos = state.todos.lock().unwrap();
  let id = todos.len() as u64 + 1;
  let todo = Todo { id, text, done: false };
  todos.push(todo.clone());
  save_todos(&state.data_dir, &todos);
  todo
}

#[tauri::command]
fn toggle_todo(id: u64, state: State<'_, AppState>) {
  let mut todos = state.todos.lock().unwrap();
  if let Some(todo) = todos.iter_mut().find(|t| t.id == id) {
    todo.done = !todo.done;
  }
  save_todos(&state.data_dir, &todos);
}

#[tauri::command]
fn delete_todo(id: u64, state: State<'_, AppState>) {
  let mut todos = state.todos.lock().unwrap();
  todos.retain(|t| t.id != id);
  save_todos(&state.data_dir, &todos);
}

fn save_todos(data_dir: &PathBuf, todos: &[Todo]) {
  let json = serde_json::to_string(todos).unwrap_or_default();
  fs::write(data_dir.join("todos.json"), json).ok();
}

fn load_todos(data_dir: &PathBuf) -> Vec<Todo> {
  fs::read_to_string(data_dir.join("todos.json"))
    .ok()
    .and_then(|content| serde_json::from_str(&content).ok())
    .unwrap_or_default()
}

fn main() {
  let data_dir = dirs::data_dir()
    .map(|d| d.join("ferrico"))
    .unwrap_or_else(|| PathBuf::from("."));
  fs::create_dir_all(&data_dir).ok();

  let todos = load_todos(&data_dir);

  tauri::Builder::default()
    .manage(AppState {
      todos: Mutex::new(todos),
      data_dir,
    })
    .invoke_handler(tauri::generate_handler![
      get_todos,
      add_todo,
      toggle_todo,
      delete_todo
    ])
    .run(tauri::generate_context!())
    .expect("error while running tauri application");
}
