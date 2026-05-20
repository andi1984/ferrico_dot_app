import { useState, useEffect } from 'react'
import { invoke } from '@tauri-apps/api/core'

interface Todo {
  id: number
  text: string
  done: boolean
}

export default function App() {
  const [todos, setTodos] = useState<Todo[]>([])
  const [input, setInput] = useState('')

  useEffect(() => {
    loadTodos()
  }, [])

  const loadTodos = async () => {
    const data = await invoke<Todo[]>('get_todos')
    setTodos(data)
  }

  const addTodo = async () => {
    if (!input.trim()) return
    await invoke('add_todo', { text: input })
    setInput('')
    loadTodos()
  }

  const toggleTodo = async (id: number) => {
    await invoke('toggle_todo', { id })
    loadTodos()
  }

  const deleteTodo = async (id: number) => {
    await invoke('delete_todo', { id })
    loadTodos()
  }

  return (
    <div className="min-h-screen bg-gradient-to-br from-blue-50 to-indigo-100 p-8">
      <div className="max-w-md mx-auto">
        <h1 className="text-3xl font-bold text-gray-800 mb-8">Todo App</h1>

        <div className="flex gap-2 mb-6">
          <input
            type="text"
            value={input}
            onChange={(e) => setInput(e.target.value)}
            onKeyDown={(e) => e.key === 'Enter' && addTodo()}
            placeholder="Add a new todo..."
            className="flex-1 px-4 py-2 border border-gray-300 rounded-lg focus:outline-none focus:ring-2 focus:ring-indigo-500"
          />
          <button
            onClick={addTodo}
            className="px-4 py-2 bg-indigo-600 text-white rounded-lg hover:bg-indigo-700 transition"
          >
            Add
          </button>
        </div>

        <div className="space-y-2">
          {todos.map((todo) => (
            <div
              key={todo.id}
              className="flex items-center gap-3 p-3 bg-white rounded-lg shadow hover:shadow-md transition"
            >
              <input
                type="checkbox"
                checked={todo.done}
                onChange={() => toggleTodo(todo.id)}
                className="w-5 h-5 text-indigo-600 rounded cursor-pointer"
              />
              <span
                className={`flex-1 ${
                  todo.done ? 'line-through text-gray-400' : 'text-gray-800'
                }`}
              >
                {todo.text}
              </span>
              <button
                onClick={() => deleteTodo(todo.id)}
                className="px-3 py-1 text-sm text-red-600 hover:bg-red-50 rounded transition"
              >
                Delete
              </button>
            </div>
          ))}
        </div>

        {todos.length === 0 && (
          <p className="text-center text-gray-400 mt-8">No todos yet</p>
        )}
      </div>
    </div>
  )
}
