/** Short broken programs that show off the teaching cards (admin preview, playground). */
export const SAMPLES: Array<{ id: string; label: string; code: string }> = [
  { id: 'semicolon', label: '少了分號', code: '#include <stdio.h>\n\nint main(void) {\n    int a = 1\n    printf("%d\\n", a);\n    return 0;\n}\n' },
  { id: 'scanf', label: 'scanf 少了 &', code: '#include <stdio.h>\n\nint main(void) {\n    int a, b;\n    scanf("%d %d", &a, b);\n    printf("%d\\n", a + b);\n    return 0;\n}\n' },
  { id: 'undeclared', label: '未宣告的變數', code: '#include <stdio.h>\n\nint main(void) {\n    for (int i = 1; i <= 5; i++) {\n        sum = sum + i;\n    }\n    printf("%d\\n", sum);\n    return 0;\n}\n' },
  { id: 'printf', label: 'printf 拼錯', code: '#include <stdio.h>\n\nint main(void) {\n    Printf("hello\\n");\n    return 0;\n}\n' },
  { id: 'brace', label: '少了大括號', code: '#include <stdio.h>\n\nint main(void) {\n    for (int i = 0; i < 3; i++) {\n        printf("%d\\n", i);\n    return 0;\n}\n' },
  { id: 'while-scanf', label: 'while(scanf(...)) 迴圈', code: '#include <stdio.h>\n\nint main(void) {\n    int x, sum = 0;\n    while (scanf("%d", &x)) {\n        sum += x;\n    }\n    printf("%d\\n", sum);\n    return 0;\n}\n' },
  { id: 'fullwidth', label: '全形分號', code: '#include <stdio.h>\n\nint main(void) {\n    int a = 5；\n    printf("%d\\n", a);\n    return 0;\n}\n' },
]
